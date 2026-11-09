import {
  SESSION_NOTICES_MAX,
  type SessionNotice,
} from "../domain/acpProjection";
import { isRecord, safeText } from "./acpDesktopGuards";

const LEVELS = new Set(["info", "warning", "error"]);

function safeNotice(value: unknown): SessionNotice | null {
  if (!isRecord(value) || !LEVELS.has(value.level as string)) return null;
  const text = safeText(value.text, 1024);
  const source =
    value.source === null || value.source === undefined
      ? null
      : safeText(value.source, 128);
  if (!text || (value.source != null && !source)) return null;
  return { level: value.level as SessionNotice["level"], text, source };
}

/**
 * Decodes the page's session notices. They are display-only, so a malformed
 * notice is dropped rather than refusing the page, and at most the newest
 * `SESSION_NOTICES_MAX` are kept.
 */
export function safeSessionNotices(value: unknown): SessionNotice[] {
  if (!Array.isArray(value)) return [];
  return value
    .slice(-SESSION_NOTICES_MAX)
    .map(safeNotice)
    .filter((notice): notice is SessionNotice => notice !== null);
}
