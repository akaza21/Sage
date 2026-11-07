import {
  ACP_SESSION_SURFACE_LIMITS as LIMITS,
  type AcpSessionActivity,
  type AcpSessionActivityState,
  type AcpSessionNotice,
  type AcpSessionNoticeLevel,
  type AcpSessionSurface,
} from "../domain/acpSessionSurface";
import {
  isOneOf,
  isRecord,
  safeCounter,
  safeIdentifier,
  safeText,
} from "./acpDesktopGuards";

const NOTICE_LEVELS: readonly AcpSessionNoticeLevel[] = [
  "info",
  "warning",
  "error",
];
const ACTIVITY_STATES: readonly AcpSessionActivityState[] = [
  "running",
  "completed",
  "failed",
  "cancelled",
];
const ACTIVITY_KEY = /^[0-9a-f]{64}$/;

/** A present optional text must be valid; an absent one stays absent. */
function optionalText(
  value: unknown,
  maximumBytes: number,
): string | null | undefined {
  if (value === undefined) return undefined;
  return nonEmptyText(value, maximumBytes);
}

function nonEmptyText(value: unknown, maximumBytes: number): string | null {
  const text = safeText(value, maximumBytes);
  return text ? text : null;
}

function safeNotice(value: unknown): AcpSessionNotice | null {
  if (!isRecord(value) || !isOneOf(value.level, NOTICE_LEVELS)) return null;
  const text = nonEmptyText(value.text, LIMITS.noticeTextBytes);
  const source = optionalText(value.source, LIMITS.noticeSourceBytes);
  if (text === null || source === null) return null;
  return { level: value.level, text, ...(source && { source }) };
}

function safeActivity(value: unknown): AcpSessionActivity | null {
  if (
    !isRecord(value) ||
    typeof value.key !== "string" ||
    !ACTIVITY_KEY.test(value.key) ||
    !isOneOf(value.state, ACTIVITY_STATES)
  )
    return null;
  const kind = nonEmptyText(value.kind, LIMITS.activityKindBytes);
  const label = nonEmptyText(value.label, LIMITS.activityLabelBytes);
  const progress = optionalText(value.progress, LIMITS.activityProgressBytes);
  if (kind === null || label === null || progress === null) return null;
  return {
    key: value.key,
    kind,
    label,
    state: value.state,
    ...(progress && { progress }),
  };
}

function boundedList<T>(
  value: unknown,
  maximum: number,
  decode: (entry: unknown) => T | null,
): T[] | null {
  if (!Array.isArray(value) || value.length > maximum) return null;
  const decoded = value.map(decode);
  return decoded.every((entry): entry is T => entry !== null) ? decoded : null;
}

/**
 * Decodes the display-only session surface of `currentSessionId`. Anything
 * malformed, over a cap, or owned by another session decodes to null.
 */
export function safeSessionSurface(
  value: unknown,
  currentSessionId: string | null,
): AcpSessionSurface | null {
  if (!isRecord(value) || currentSessionId === null) return null;
  const sessionId = safeIdentifier(value.sessionId);
  const revision = safeCounter(value.revision, true);
  const title = optionalText(value.title, LIMITS.titleBytes);
  const modelId = optionalText(value.modelId, LIMITS.modelIdBytes);
  const notices = boundedList(value.notices, LIMITS.notices, safeNotice);
  const activities = boundedList(
    value.activities,
    LIMITS.activities,
    safeActivity,
  );
  if (
    sessionId !== currentSessionId ||
    revision === null ||
    title === null ||
    modelId === null ||
    notices === null ||
    activities === null
  )
    return null;
  return {
    sessionId: sessionId as AcpSessionSurface["sessionId"],
    ...(title && { title }),
    notices,
    ...(modelId && { modelId }),
    activities,
    revision: revision as AcpSessionSurface["revision"],
  };
}
