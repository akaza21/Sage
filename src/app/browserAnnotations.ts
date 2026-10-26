/**
 * Where Annotate's `browserElement` items leave the browser layer
 * (docs/work-items/BROWSER-FIRST-CLASS/contracts/browser-element-context-v1.md).
 * Rust builds, redacts and checks each item; this module checks the mirrored
 * limits again and hands the item to the sink the Chat track registers.
 *
 * TODO(chat: browser-element-context-v1 §7): register a sink whose `attach`
 * calls `captureChatOwnership` then `attachChatContext`, and whose
 * `nextOrdinal` reads the draft, once `attachedContext.ts` parses
 * `browserElement`. Until then no sink is registered and the comment box
 * says why it cannot attach.
 */

import { utf8ByteLength } from "../domain/text";

export interface BrowserElementContext {
  kind: "browserElement";
  target: "element";
  ordinal: number;
  tabId: string;
  url: string;
  title: string;
  selector: string;
  role: string;
  name: string;
  text: string;
  box: { x: number; y: number; width: number; height: number };
  comment: string;
}

export interface BrowserElementSink {
  /** The next free element number in the draft (1–8), or null when none is free. */
  nextOrdinal(): number | null;
  attach(element: BrowserElementContext): void;
}

let sink: BrowserElementSink | null = null;

/** Registers the composer's sink; the returned function unregisters it. */
export function registerBrowserElementSink(next: BrowserElementSink) {
  sink = next;
  return () => {
    if (sink === next) sink = null;
  };
}

export function browserElementSink(): BrowserElementSink | null {
  return sink;
}

const KEYS = [
  "kind",
  "target",
  "ordinal",
  "tabId",
  "url",
  "title",
  "selector",
  "role",
  "name",
  "text",
  "box",
  "comment",
].sort();
// Control characters other than a newline, and bidi controls.
const UNSAFE =
  // eslint-disable-next-line no-control-regex
  /[\u0000-\u0009\u000b-\u001f\u007f-\u009f\u061c\u200e\u200f\u2028\u2029\u202a-\u202e\u2066-\u2069]/u;

function text(value: unknown, limit: number, multiline = false) {
  return (
    typeof value === "string" &&
    utf8ByteLength(value) <= limit &&
    !UNSAFE.test(value) &&
    (multiline || !value.includes("\n"))
  );
}

function integer(value: unknown, low: number, high: number) {
  return (
    Number.isInteger(value) && Number(value) >= low && Number(value) <= high
  );
}

function webUrl(value: unknown) {
  if (typeof value !== "string" || utf8ByteLength(value) > 2048) return false;
  try {
    const url = new URL(value);
    return /^https?:$/u.test(url.protocol) && !url.username && !url.password;
  } catch {
    return false;
  }
}

/** The contract's mirrored limits; redaction fixpoints are Rust's checks. */
export function parseBrowserElement(
  value: unknown,
): BrowserElementContext | null {
  if (!value || typeof value !== "object") return null;
  const item = value as Record<string, unknown>;
  const box = item.box as Record<string, unknown> | null;
  const valid =
    Object.keys(item).sort().join() === KEYS.join() &&
    item.kind === "browserElement" &&
    item.target === "element" &&
    integer(item.ordinal, 1, 8) &&
    /^browser-[1-9][0-9]*$/u.test(String(item.tabId)) &&
    webUrl(item.url) &&
    text(item.title, 256) &&
    text(item.selector, 512) &&
    item.selector !== "" &&
    /^[A-Za-z][A-Za-z0-9-]{0,63}$/u.test(String(item.role)) &&
    text(item.name, 256) &&
    text(item.text, 1024, true) &&
    typeof item.comment === "string" &&
    utf8ByteLength(item.comment) <= 4096 &&
    !!box &&
    Object.keys(box).sort().join() === "height,width,x,y" &&
    integer(box.x, 0, 10_000_000) &&
    integer(box.y, 0, 10_000_000) &&
    integer(box.width, 1, 100_000) &&
    integer(box.height, 1, 100_000) &&
    utf8ByteLength(JSON.stringify(item)) <= 8192;
  return valid ? (item as unknown as BrowserElementContext) : null;
}

/** Checks the item again and attaches it; false when it fails or no sink waits. */
export function handOffBrowserElement(value: unknown): boolean {
  const element = parseBrowserElement(value);
  if (!element || !sink) return false;
  sink.attach(element);
  return true;
}
