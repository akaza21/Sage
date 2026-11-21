import {
  SESSION_NOTICES_MAX,
  type SessionNotice,
} from "../domain/acpProjection";
import { isRecord, safeText } from "./acpDesktopGuards";

const LEVELS = ["info", "warning", "error"];

function safeNotice(value: unknown, index: number): SessionNotice | null {
  if (!isRecord(value) || !LEVELS.includes(value.level as string)) return null;
  const text = safeText(value.text, 1024);
  const source = value.source == null ? null : safeText(value.source, 128);
  const diagnostic = value.diagnostic ?? false;
  const identity =
    value.identity == null ? undefined : safeText(value.identity, 1024);
  if (
    !text ||
    (value.source != null && !source) ||
    typeof diagnostic !== "boolean" ||
    (value.identity != null && !identity)
  )
    return null;
  return {
    identity: identity ?? `legacy-record:${index}`,
    level: value.level as SessionNotice["level"],
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
