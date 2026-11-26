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

/** Bound display-only records; malformed notices never reject a page. The
 * native store keeps one notice per identity, so a repeat keeps the latest. */
export function safeSessionNotices(value: unknown): SessionNotice[] {
  if (!Array.isArray(value)) return [];
  const notices = new Map<string, SessionNotice>();
  for (const [index, record] of value.slice(-SESSION_NOTICES_MAX).entries()) {
    if (!isRecord(record) || !isOneOf(record.level, LEVELS)) continue;
    const text = safeText(record.text, 1024);
    const source = safeNullableText(record.source ?? null, 128);
    const diagnostic = record.diagnostic ?? false;
    const identity =
      record.identity == null
        ? `legacy-record:${index}`
        : safeText(record.identity, 1024);
    if (
      !text ||
      (source !== null && !source) ||
      typeof diagnostic !== "boolean" ||
      !identity
    )
      continue;
    notices.set(identity, {
      identity,
      level: record.level,
      text,
      source,
      diagnostic,
      usageLimit: record.usageLimit,
    });
  }
  return [...notices.values()];
}
