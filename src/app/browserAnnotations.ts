/**
 * Where Annotate's `browserElement` items leave the browser layer
 * (docs/work-items/BROWSER-FIRST-CLASS/contracts/browser-element-context-v1.md).
 * Rust builds, redacts and checks each item; this module checks the mirrored
 * limits again and hands the item to the registered sink, which puts it in
 * the pinned Agent Chat's draft (`browserElementChatSink.ts`).
 */

import {
  parseBrowserElementContext,
  type BrowserElementContext,
} from "../domain/attachedContext";

export {
  parseBrowserElementContext as parseBrowserElement,
  type BrowserElementContext,
};

export interface BrowserElementSink {
  /** The next free element number in the draft (1–8), or null when none is free. */
  nextOrdinal(): number | null;
  /** False when the draft refused the item. */
  attach(element: BrowserElementContext): boolean;
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

/** Checks the item again and attaches it; false when it fails or no sink takes it. */
export function handOffBrowserElement(value: unknown): boolean {
  const element = parseBrowserElementContext(value);
  return Boolean(element && sink?.attach(element));
}
