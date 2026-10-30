import { afterEach, describe, expect, it, vi } from "vitest";

import {
  requestEditorFocus,
  subscribeEditorFocusRequest,
  takeEditorFocusRequest,
} from "./editorFocusRequest";

const created = { rootId: "root-a", relativePath: "src/new.ts" };
const other = { rootId: "root-a", relativePath: "src/old.ts" };

afterEach(() => {
  takeEditorFocusRequest(created);
  vi.useRealTimers();
});

describe("editor focus request", () => {
  it("is taken once and tells subscribers", () => {
    const listener = vi.fn();
    const unsubscribe = subscribeEditorFocusRequest(listener);
    expect(takeEditorFocusRequest(created)).toBe(false);
    requestEditorFocus(created);
    expect(listener).toHaveBeenCalledOnce();
    expect(takeEditorFocusRequest(created)).toBe(true);
    expect(takeEditorFocusRequest(created)).toBe(false);
    unsubscribe();
    requestEditorFocus(created);
    expect(listener).toHaveBeenCalledOnce();
  });

  it("is taken only by the File it names", () => {
    requestEditorFocus(created);
    expect(takeEditorFocusRequest(other)).toBe(false);
    expect(takeEditorFocusRequest({ ...created })).toBe(true);
  });

  it("expires when no editor takes it soon", () => {
    vi.useFakeTimers();
    requestEditorFocus(created);
    vi.advanceTimersByTime(5_000);
    expect(takeEditorFocusRequest(created)).toBe(false);
  });
});
