import type { BrowserPermissionKind } from "../../ports/nativeBrowserPort";
import type {
  BrowserSiteSetting,
  BrowserSiteSettingsView,
} from "../../ports/nativeBrowserSiteSettingsPort";

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

const ORIGIN = /^https?:\/\/[^/?#\s]+$/;

function isOrigin(value: unknown): value is string {
  return (
    typeof value === "string" &&
    value.length <= MAX_ORIGIN_LENGTH &&
    ORIGIN.test(value)
  );
}

function admitOne(input: unknown): BrowserSiteSetting | null {
  if (typeof input !== "object" || input === null) return null;
  const { origin, kind, decision } = input as Record<string, unknown>;
  if (
    !isOrigin(origin) ||
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

/** Keeps only well-formed site origins, bounded to the store's cap. */
export function admitSiteOrigins(input: unknown): string[] {
  if (!Array.isArray(input)) return [];
  return input.slice(0, MAX_SITE_SETTINGS).filter(isOrigin);
}

/** Admits Settings' view: the saved list and the sites with stale tabs. */
export function admitSiteSettingsView(input: unknown): BrowserSiteSettingsView {
  const source =
    typeof input === "object" && input !== null
      ? (input as Record<string, unknown>)
      : {};
  return {
    settings: admitBrowserSiteSettings(source.settings),
    staleSites: admitSiteOrigins(source.staleSites),
  };
}
