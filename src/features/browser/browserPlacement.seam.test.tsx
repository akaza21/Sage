import { cleanup, renderHook } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { useBrowserPlacement } from "./useBrowserPlacement";

let output: { command: string; payload: unknown }[];
beforeEach(() => {
  output = [];
  mockIPC((command, payload) => {
    output.push({ command, payload });
  });
  vi.stubGlobal(
    "ResizeObserver",
    class {
      observe() {}
      disconnect() {}
    },
  );
  vi.stubGlobal("requestAnimationFrame", (callback: FrameRequestCallback) => {
    callback(0);
    return 1;
  });
});
afterEach(() => {
  cleanup();
  document.body.replaceChildren();
  clearMocks();
  vi.unstubAllGlobals();
});

it("emits only changed native bounds across sibling and ancestor scrolling", async () => {
  const canvas = document.createElement("section");
  const frame = document.createElement("div");
  const transcript = document.createElement("div");
  canvas.append(frame);
  document.body.append(canvas, transcript);
  let rect = new DOMRect(600, 50, 300, 200);
  frame.getBoundingClientRect = () => rect;
  const reportError = () => {};
  renderHook(() =>
    useBrowserPlacement(
      "preview",
      { current: frame },
      { current: null },
      reportError,
    ),
  );
  // Wait for the actual native port's dynamic import and IPC delivery.
  await vi.waitFor(() => expect(output).toHaveLength(1));
  rect = new DOMRect(600, 70, 300, 200);
  transcript.dispatchEvent(new Event("scroll"));
  await new Promise((resolve) => setTimeout(resolve, 0));
  expect(output).toEqual([
    {
      command: "browser_set_bounds",
      payload: { tabId: "preview", x: 600, y: 50, width: 300, height: 200 },
    },
  ]);
  canvas.dispatchEvent(new Event("scroll"));
  await vi.waitFor(() => expect(output).toHaveLength(2));
  window.dispatchEvent(new Event("resize"));
  canvas.dispatchEvent(new Event("scroll"));
  await new Promise((resolve) => setTimeout(resolve, 0));
  expect(output).toEqual([
    {
      command: "browser_set_bounds",
      payload: { tabId: "preview", x: 600, y: 50, width: 300, height: 200 },
    },
    {
      command: "browser_set_bounds",
      payload: { tabId: "preview", x: 600, y: 70, width: 300, height: 200 },
    },
  ]);
});
