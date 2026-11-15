import type {
  AcpSessionSurface,
  SessionActivity,
} from "../domain/acpSessionSurface";
import { isOneOf, isRecord, safeText } from "./acpDesktopGuards";
import { invokeAcp } from "./acpDesktopCommand";

const MAX_ROWS = 16;
const TURN_ID = /^turn-[0-9a-f-]{36}$/;

function activity(value: unknown): SessionActivity | null {
  if (
    !isRecord(value) ||
    !isOneOf(value.state, ["running", "completed", "failed", "cancelled"]) ||
    typeof value.key !== "string" ||
    !/^[0-9a-f]{24}$/.test(value.key)
  )
    return null;
  const kind = safeText(value.kind, 64);
  const label = safeText(value.label, 256);
  const progress =
    value.progress == null ? null : safeText(value.progress, 512);
  if (!kind || !label || (value.progress != null && !progress)) return null;
  return {
    key: value.key,
    kind,
    label,
    state: value.state,
    progress,
    stoppable: value.stoppable === true,
  };
}

/** Display-only: a malformed surface is dropped, never trusted. */
export function decodeSessionSurface(
  value: unknown,
): AcpSessionSurface | undefined {
  if (
    !isRecord(value) ||
    !Array.isArray(value.activities) ||
    value.activities.length > MAX_ROWS
  )
    return undefined;
  const activities = value.activities.map(activity);
  if (activities.includes(null)) return undefined;
  return {
    activities: activities as SessionActivity[],
    ...(typeof value.undoTurn === "string" && TURN_ID.test(value.undoTurn)
      ? {
          undoTurn: value.undoTurn,
          ...(value.undoKeepsFiles === true ? { undoKeepsFiles: true } : {}),
        }
      : {}),
  };
}

export type SessionControlIntent =
  | { kind: "undoTurn"; turnId: string }
  | { kind: "stopActivity"; activityKey: string };

export interface SessionControlRequest {
  projectId: string;
  projectGeneration: string;
  chatId: string;
  sessionId: string;
  runtimeEpoch: string;
  control: SessionControlIntent;
}

export async function controlAcpSession(
  request: SessionControlRequest,
): Promise<string | null> {
  const value = await invokeAcp<unknown, SessionControlRequest>(
    "control_acp_session",
    request,
  );
  return isRecord(value) ? safeText(value.summary, 512) : null;
}
