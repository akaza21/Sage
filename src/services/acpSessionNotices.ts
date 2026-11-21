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

/** Notices are display-only, so a malformed one is dropped, not the page. */
export function safeSessionNotices(value: unknown): SessionNotice[] {
  if (!Array.isArray(value)) return [];
  return value
    .slice(-SESSION_NOTICES_MAX)
    .map(safeNotice)
    .filter((notice): notice is SessionNotice => notice !== null);
}
