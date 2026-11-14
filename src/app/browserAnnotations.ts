/**
 * Contract: docs/work-items/BROWSER-FIRST-CLASS/contracts/browser-element-
 * context-v1.md. Rust builds, redacts and checks each item; this checks the
 * mirrored limits again.
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
  nextOrdinal(): number | null;
  attach(element: BrowserElementContext): boolean;
}

let sink: BrowserElementSink | null = null;

export function registerBrowserElementSink(next: BrowserElementSink) {
  sink = next;
  return () => {
    if (sink === next) sink = null;
  };
}

export function browserElementSink(): BrowserElementSink | null {
  return sink;
}

export function handOffBrowserElement(value: unknown): boolean {
  const element = parseBrowserElementContext(value);
  return Boolean(element && sink?.attach(element));
}
