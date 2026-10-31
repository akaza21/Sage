import type { BrowserPermissionKind } from "../../ports/nativeBrowserPort";
import type { BrowserSiteSetting } from "../../ports/nativeBrowserSiteSettingsPort";

/** Rust keeps at most this many saved answers (`MAX_SITE_PERMISSIONS`). */
export const MAX_SITE_SETTINGS = 256;
const MAX_ORIGIN_LENGTH = 512;

export const SITE_PERMISSION_LABELS: Readonly<
  Record<BrowserPermissionKind, string>
> = Object.freeze({
  camera: "Camera",
  microphone: "Microphone",
  geolocation: "Location",
  notifications: "Notifications",
  clipboardRead: "Clipboard",
});

function isKind(value: unknown): value is BrowserPermissionKind {
  return (
    typeof value === "string" &&
    Object.prototype.hasOwnProperty.call(SITE_PERMISSION_LABELS, value)
  );
}

function admitOne(input: unknown): BrowserSiteSetting | null {
  if (typeof input !== "object" || input === null) return null;
  const { origin, kind, decision } = input as Record<string, unknown>;
  if (
    typeof origin !== "string" ||
    origin.length > MAX_ORIGIN_LENGTH ||
    !/^https?:\/\/[^/?#\s]+$/.test(origin) ||
    !isKind(kind) ||
    (decision !== "allow" && decision !== "deny")
  ) {
    return null;
  }
  return { origin, kind, decision };
}

/**
 * Keeps only well-formed rows from Rust's saved site answers, bounded to the
 * store's cap. A malformed payload yields no rows rather than a guess.
 */
export function admitBrowserSiteSettings(input: unknown): BrowserSiteSetting[] {
  if (!Array.isArray(input)) return [];
  const rows: BrowserSiteSetting[] = [];
  for (const item of input.slice(0, MAX_SITE_SETTINGS)) {
    const row = admitOne(item);
    if (row) rows.push(row);
  }
  return rows;
}
