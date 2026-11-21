import {
  SESSION_NOTICES_MAX,
  type SessionNotice,
} from "../domain/acpProjection";
import {
  isOneOf,
  isRecord,
  safeNullableText,
  safeText,
} from "./acpDesktopGuards";

const LEVELS = ["info", "warning", "error"] as const;

function safeNotice(value: unknown, index: number): SessionNotice | null {
  if (!isRecord(value) || !isOneOf(value.level, LEVELS)) return null;
  const text = safeText(value.text, 1024);
  const source = safeNullableText(value.source ?? null, 128);
  const diagnostic = value.diagnostic ?? false;
  const identity =
    value.identity == null
      ? `legacy-record:${index}`
      : safeText(value.identity, 1024);
  if (
    !text ||
    (source !== null && !source) ||
    typeof diagnostic !== "boolean" ||
    !identity
  )
    return null;
  return {
    identity,
    level: value.level,
    text,
    source,
    diagnostic,
  };
}

/** Bound and group display-only records; malformed notices never reject a page. */
export function safeSessionNotices(value: unknown): SessionNotice[] {
  if (!Array.isArray(value)) return [];
  const notices = new Map<string, SessionNotice>();
  for (const [index, record] of value.slice(-SESSION_NOTICES_MAX).entries()) {
    const notice = safeNotice(record, index);
    if (!notice) continue;
    const previous = notices.get(notice.identity!);
    if (
      !previous ||
      LEVELS.indexOf(notice.level) >= LEVELS.indexOf(previous.level)
    )
      notices.set(notice.identity!, notice);
  }
  return [...notices.values()];
}
