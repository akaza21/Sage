import type { AcpChatPage, TurnProjection } from "../../../domain/acp";
import { acpDesktop } from "../../../services/acpDesktop";
import { utf8ByteLength } from "../../../domain/text";

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

export const MAX_TURN_WORKSPACE_CHANGES = 300;
const MAX_FETCHED_TURNS = 5;
const MAX_CACHED_TURNS = 64;
const MAX_PATH_BYTES = 16 * 1024;
const TERMINAL_TURN_STATES: ReadonlySet<string> = new Set([
  "completed",
  "interrupted",
  "cancelled",
  "failed",
  "deliveryUnknown",
]);

const cache = new Map<string, readonly TurnWorkspaceChange[]>();
const inFlight = new Set<string>();

function canonicalRelativePath(value: unknown): value is string {
  return (
    typeof value === "string" &&
    value.length > 0 &&
    utf8ByteLength(value) <= MAX_PATH_BYTES &&
    !value.includes("\0") &&
    !value.includes("\\") &&
    !value.startsWith("/") &&
    value.charAt(1) !== ":" &&
    value
      .split("/")
      .every((part) => part.length > 0 && part !== "." && part !== "..")
  );
}

/** Decodes the native reply, rejecting anything unbounded or unsafe. */
export function decodeTurnWorkspaceChanges(
  value: unknown,
  turnId: string,
): TurnWorkspaceChange[] | null {
  if (typeof value !== "object" || value === null) return null;
  const record = value as Record<string, unknown>;
  if (
    typeof record.rootId !== "string" ||
    record.rootId.length === 0 ||
    typeof record.tracked !== "boolean" ||
    !Array.isArray(record.changes) ||
    record.changes.length > MAX_TURN_WORKSPACE_CHANGES
  )
    return null;
  const rootId = record.rootId;
  const changes: TurnWorkspaceChange[] = [];
  for (const entry of record.changes) {
    if (typeof entry !== "object" || entry === null) return null;
    const change = entry as Record<string, unknown>;
    if (
      !canonicalRelativePath(change.relativePath) ||
      !["created", "updated", "deleted"].includes(String(change.operation))
    )
      return null;
    changes.push({
      turnId,
      rootId,
      relativePath: change.relativePath,
      operation: change.operation as TurnWorkspaceChange["operation"],
    });
  }
  return changes;
}

function remember(turnId: string, changes: readonly TurnWorkspaceChange[]) {
  cache.delete(turnId);
  cache.set(turnId, changes);
  if (cache.size > MAX_CACHED_TURNS) cache.delete(cache.keys().next().value!);
}

function candidateTurns(page: AcpChatPage): TurnProjection[] {
  return page.turns
    .filter(
      (turn) =>
        turn.sessionId === page.currentSessionId &&
        TERMINAL_TURN_STATES.has(turn.state),
    )
    .slice(-MAX_FETCHED_TURNS);
}

/**
 * Loads the settled workspace changes of the page's latest terminal turns
 * (at most a few, each fetched once) and resolves with every cached change.
 */
export async function loadTurnWorkspaceChanges(
  page: AcpChatPage,
  projectGeneration: string,
): Promise<readonly TurnWorkspaceChange[]> {
  const turns = candidateTurns(page);
  await Promise.all(
    turns
      .filter((turn) => !cache.has(turn.id) && !inFlight.has(turn.id))
      .map(async (turn) => {
        inFlight.add(turn.id);
        try {
          const changes = decodeTurnWorkspaceChanges(
            await acpDesktop.getTurnWorkspaceChanges({
              projectId: page.projectId,
              projectGeneration,
              chatId: page.chatId,
              turnId: turn.id,
            }),
            turn.id,
          );
          if (changes) remember(turn.id, changes);
        } catch {
          // A failed read leaves the turn uncached; it is retried later.
        } finally {
          inFlight.delete(turn.id);
        }
      }),
  );
  return turns.flatMap((turn) => cache.get(turn.id) ?? []);
}
export function resetTurnWorkspaceChangesForTest() {
  cache.clear();
  inFlight.clear();
}
