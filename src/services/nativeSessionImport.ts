import type { ProjectLayoutRequest } from "../domain/chat";
import type { ProjectTransitionResult } from "../domain/project";
import { invokeNative } from "../ports/nativeInvoke";
import {
  turnsForView,
  type TranscriptOwner,
  type TranscriptTurn,
} from "./acpChildTranscript";
import { isRecord, safeIdentifier } from "./acpDesktopGuards";

/** A native session started outside Sage that a new Chat can continue. */
export interface NativeSessionSummary {
  threadId: string;
  title: string;
  /** Unix seconds of its latest activity. */
  updatedAt: number;
}

export interface NativeSessionsPage {
  sessions: NativeSessionSummary[];
  nextCursor: string | null;
}

const PAGE_SIZE = 25;

function invalid(): never {
  throw new Error("Invalid native session response");
}

/** One page of the Project's importable sessions, newest activity first. */
export async function listNativeSessions(
  projectId: string,
  cursor: string | null,
): Promise<NativeSessionsPage> {
  const value = await invokeNative<unknown>("list_native_sessions", {
    projectId,
    cursor,
  });
  if (
    !isRecord(value) ||
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
  };
}

export function importNativeSession(request: {
  currentLayout: ProjectLayoutRequest;
  threadId: string;
  title: string;
  expectedTransitionEpoch: number;
}): Promise<ProjectTransitionResult> {
  return invokeNative("import_native_session", { ...request });
}

export interface ImportedHistory {
  turns: TranscriptTurn[];
  retentionLimited: boolean;
}

/** The history an imported Chat continues from, in the shared item shapes. */
export async function readImportedHistory(
  owner: TranscriptOwner,
): Promise<ImportedHistory | null> {
  const value = await invokeNative<unknown>("read_imported_history", {
    projectId: owner.projectId,
    chatId: owner.chatId,
  });
  if (value === null) return null;
  const turns = isRecord(value) ? turnsForView(value.turns, owner) : null;
  if (!isRecord(value) || !turns || typeof value.retentionLimited !== "boolean")
    invalid();
  return { turns, retentionLimited: value.retentionLimited };
}
