import {
  isOneOf,
  isRecord,
  safeText,
} from "../../../services/acpDesktopGuards";

/** Native caps; the session surface enforces the same ones. */
const STATUSES_MAX = 8;
const WIDGETS_MAX = 4;
const LINES_MAX = 10;
const KEY_BYTES = 128;
const LINE_BYTES = 200;
const EDITOR_TEXT_BYTES = 16 * 1024;
const TITLE_BYTES = 256;

export interface ChromeWidget {
  key: string;
  placement: "above" | "below";
  lines: readonly string[];
  hiddenLines: number;
}

export interface SessionChrome {
  statuses: readonly { key: string; text: string }[];
  widgets: readonly ChromeWidget[];
  /** `key` names this offer, so a handled one is never offered again. */
  editorText?: { key: string; text: string };
  subtitle?: string;
}

const list = (value: unknown, max: number) =>
  value === undefined
    ? []
    : Array.isArray(value) && value.length <= max
      ? value
      : null;

function widget(value: unknown): ChromeWidget | null {
  if (!isRecord(value) || !isOneOf(value.placement, ["above", "below"]))
    return null;
  const key = safeText(value.key, KEY_BYTES);
  const lines = list(value.lines, LINES_MAX);
  const hiddenLines = value.hiddenLines ?? 0;
  if (
    !key ||
    !lines ||
    !Number.isSafeInteger(hiddenLines) ||
    (hiddenLines as number) < 0 ||
    lines.some((line) => safeText(line, LINE_BYTES) === null)
  )
    return null;
  return {
    key,
    placement: value.placement,
    lines: lines as string[],
    hiddenLines: hiddenLines as number,
  };
}

/** Display-only: a malformed chrome is dropped whole, never trusted. */
export function decodeSessionChrome(surface: unknown): SessionChrome | null {
  const value = isRecord(surface) ? surface.chrome : undefined;
  if (!isRecord(value)) return null;
  const statuses = list(value.statuses, STATUSES_MAX)?.map((status) =>
    isRecord(status) &&
    safeText(status.key, KEY_BYTES) &&
    safeText(status.text, 200)
      ? { key: status.key as string, text: status.text as string }
      : null,
  );
  const widgets = list(value.widgets, WIDGETS_MAX)?.map(widget);
  const offer = value.editorText;
  const editorText =
    isRecord(offer) &&
    safeText(offer.key, 64) &&
    safeText(offer.text, EDITOR_TEXT_BYTES)
      ? { key: offer.key as string, text: offer.text as string }
      : undefined;
  const subtitle = safeText(value.subtitle, TITLE_BYTES) || undefined;
  if (
    !statuses ||
    !widgets ||
    statuses.includes(null) ||
    widgets.includes(null) ||
    (offer !== undefined && !editorText) ||
    (value.subtitle !== undefined && !subtitle)
  )
    return null;
  return {
    statuses: statuses as SessionChrome["statuses"],
    widgets: widgets as ChromeWidget[],
    ...(editorText && { editorText }),
    ...(subtitle && { subtitle }),
  };
}
