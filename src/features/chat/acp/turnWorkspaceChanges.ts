import type { AcpChatPage } from "../../../domain/acp";
import { isRecord, isShortId } from "../../../domain/guards";
import { acpDesktop } from "../../../services/acpDesktop";
import { canonicalRelativePath } from "./agentEditSummary";
import { terminalTurnStates } from "./statusSemantics";

/**
 * AP-B B5: workspace (Git) changes of terminal turns, computed and owned by
 * Rust. Edits made by shell commands never reach harness fileChange items;
 * this disposable cache only renders what the native ledger reports.
 */
export interface TurnWorkspaceChange {
  turnId: string;
  rootId: string;
  relativePath: string;
  operation: "created" | "updated" | "deleted";
}

export interface TurnWorkspaceLedger {
  changes: readonly TurnWorkspaceChange[];
  incompleteTurnIds: ReadonlySet<string>;
}

interface TurnWorkspaceResult {
  changes: readonly TurnWorkspaceChange[];
  incomplete: boolean;
}

export const MAX_TURN_WORKSPACE_CHANGES = 300;
const MAX_CONCURRENT_READS = 5;
const MAX_CACHED_TURNS = 100;

const cache = new Map<string, TurnWorkspaceResult>();
const inFlight = new Map<string, { result?: TurnWorkspaceResult }>();
const waiters = new Set<() => void>();
const unavailable: TurnWorkspaceResult = { changes: [], incomplete: true };

/** Decodes the native reply, rejecting anything unbounded or unsafe. */
export function decodeTurnWorkspaceChanges(
  record: unknown,
  turnId: string,
): TurnWorkspaceResult | null {
  if (!isRecord(record)) return null;
  if (
    !isShortId(record.rootId) ||
    typeof record.tracked !== "boolean" ||
    !Array.isArray(record.changes) ||
    record.changes.length > MAX_TURN_WORKSPACE_CHANGES ||
    record.changes.some(
      (change) =>
        !isRecord(change) ||
        !canonicalRelativePath(change.relativePath) ||
        !["created", "updated", "deleted"].includes(change.operation as string),
    )
  )
    return null;
  return {
    changes: record.tracked
      ? record.changes.map((change) => ({
          turnId,
          rootId: record.rootId as string,
          relativePath: change.relativePath,
          operation: change.operation as TurnWorkspaceChange["operation"],
        }))
      : [],
    incomplete: !record.tracked || record.truncated !== false,
  };
}

async function loadTurn(
  request: Parameters<typeof acpDesktop.getTurnWorkspaceChanges>[0],
  signal: AbortSignal,
): Promise<TurnWorkspaceResult> {
  const key = JSON.stringify(request);
  while (true) {
    if (signal.aborted || waiters.size >= 32) return unavailable;
    const retained = cache.get(key);
    if (retained) return retained;
    let pending = inFlight.get(key);
    const changed = new Promise<void>((resolve) => {
      const done = () => {
        waiters.delete(done);
        signal.removeEventListener("abort", done);
        resolve();
      };
      waiters.add(done);
      signal.addEventListener("abort", done);
    });
    if (!pending && inFlight.size < MAX_CONCURRENT_READS) {
      pending = {};
      inFlight.set(key, pending);
      void acpDesktop
        .getTurnWorkspaceChanges(request)
        .then(
          (reply) =>
            decodeTurnWorkspaceChanges(reply, request.turnId) ?? unavailable,
        )
        .catch(() => unavailable)
        .then((result) => {
          if (!result.incomplete) {
            if (cache.set(key, result).size > MAX_CACHED_TURNS)
              cache.delete(cache.keys().next().value!);
          }
          pending!.result = result;
          inFlight.delete(key);
          for (const notify of waiters) notify();
        });
    }
    await changed;
    if (pending?.result) return pending.result;
  }
}

export default async function loadTurnWorkspaceChanges(
  page: AcpChatPage,
  projectGeneration: string,
  signal: AbortSignal,
): Promise<TurnWorkspaceLedger> {
  const turns = page.turns
    .filter(
      (turn) =>
        page.sessions.some((session) => session.id === turn.sessionId) &&
        terminalTurnStates.has(turn.state),
    )
    .slice(-MAX_CACHED_TURNS);
  const changes: TurnWorkspaceChange[] = [];
  const incompleteTurnIds = new Set<string>();
  while (turns.length) {
    await Promise.all(
      turns.splice(-MAX_CONCURRENT_READS).map((turn) =>
        loadTurn(
          {
            projectId: page.projectId,
            projectGeneration,
            chatId: page.chatId,
            turnId: turn.id,
          },
          signal,
        ).then((result) => {
          changes.push(...result.changes);
          if (result.incomplete) incompleteTurnIds.add(turn.id);
        }),
      ),
    );
  }
  return { changes, incompleteTurnIds };
}
export function resetTurnWorkspaceChangesForTest() {
  cache.clear();
  inFlight.clear();
}
