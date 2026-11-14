import type { AcpItem, TurnProjection } from "../domain/acp";
import type { ProjectLayoutRequest } from "../domain/chat";
import type { ProjectTransitionResult } from "../domain/project";
import { invokeNative } from "../ports/nativeInvoke";
import { turnsForView } from "./acpChildTranscript";
import { isRecord, safeIdentifier } from "./acpDesktopGuards";

export interface NativeSessionSummary {
  threadId: string;
  title: string;
  /** Unix seconds of its latest activity. */
  updatedAt: number;
}

/** The runtime that listed a page; only it reads one of its sessions. */
export interface ImportLister {
  sessionId: string;
  runtimeEpoch: string;
}

export interface NativeSessionsPage {
  sessions: NativeSessionSummary[];
  nextCursor: string | null;
  lister: ImportLister;
}

const PAGE_SIZE = 25;

/** The sessions a runtime remembers from its listings (its own
 * `LISTED_SESSIONS_MAX`); it imports only those. */
const LISTED_SESSIONS_MAX = 256;

export function canListMoreSessions(shown: number): boolean {
  return shown + PAGE_SIZE <= LISTED_SESSIONS_MAX;
}

function invalid(): never {
  throw new Error("Invalid native session response");
}

export async function listNativeSessions(
  projectId: string,
  cursor: string | null,
  lister: ImportLister | null,
): Promise<NativeSessionsPage> {
  const value = await invokeNative<unknown>("list_native_sessions", {
    projectId,
    cursor,
    lister,
  });
  const listed = isRecord(value) && value.lister;
  if (
    !isRecord(value) ||
    !isRecord(listed) ||
    !safeIdentifier(listed.sessionId) ||
    typeof listed.runtimeEpoch !== "string" ||
    !Array.isArray(value.sessions) ||
    value.sessions.length > PAGE_SIZE ||
    (value.nextCursor !== null && typeof value.nextCursor !== "string")
  )
    invalid();
  return {
    sessions: value.sessions.map((session: unknown) => {
      const threadId = isRecord(session) && safeIdentifier(session.threadId);
      if (
        !isRecord(session) ||
        !threadId ||
        typeof session.title !== "string" ||
        !Number.isSafeInteger(session.updatedAt)
      )
        invalid();
      return {
        threadId,
        title: session.title.slice(0, 200),
        updatedAt: session.updatedAt as number,
      };
    }),
    nextCursor: value.nextCursor,
    lister: {
      sessionId: listed.sessionId,
      runtimeEpoch: listed.runtimeEpoch,
    } as ImportLister,
  };
}

export function importNativeSession(request: {
  currentLayout: ProjectLayoutRequest;
  lister: ImportLister;
  threadId: string;
  title: string;
  expectedTransitionEpoch: number;
}): Promise<ProjectTransitionResult> {
  return invokeNative("import_native_session", { ...request });
}

export interface ImportedHistory {
  turns: TurnProjection[];
  retentionLimited: boolean;
}

// History never runs again here: unfinished work shows as stopped.
const unfinished = new Set(["started", "streaming", "pending", "inProgress"]);

export async function readImportedHistory(
  projectId: string,
  chatId: string,
): Promise<ImportedHistory | null> {
  const value = await invokeNative<unknown>("read_imported_history", {
    projectId,
    chatId,
  });
  if (value === null) return null;
  if (!isRecord(value)) invalid();
  const owner = {
    projectId,
    chatId,
    sessionId: value.sessionId as string,
    runtimeEpoch: value.runtimeEpoch as string,
  };
  const turns = turnsForView(value.turns, owner);
  if (!turns || typeof value.retentionLimited !== "boolean") invalid();
  return {
    retentionLimited: value.retentionLimited,
    turns: turns.map((turn, index) => ({
      id: turn.id as TurnProjection["id"],
      sessionId: owner.sessionId as TurnProjection["sessionId"],
      runtimeEpoch: owner.runtimeEpoch as TurnProjection["runtimeEpoch"],
      ordinal: index + 1,
      state:
        turn.status === "inProgress"
          ? "interrupted"
          : (turn.status as TurnProjection["state"]),
      revision: "0" as TurnProjection["revision"],
      createdAt: 0,
      updatedAt: 0,
      terminalAt: null,
      items: turn.items.map(
        (item) =>
          ({
            ...item,
            lifecycle: unfinished.has(item.lifecycle)
              ? "cancelled"
              : item.lifecycle,
            toolState: unfinished.has(item.toolState!)
              ? "cancelled"
              : item.toolState,
          }) as AcpItem,
      ),
      activities: [],
      browserOperations: [],
      browserOperationsCollapsed: 0,
    })),
  };
}
