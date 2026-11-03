import { cleanup, renderHook } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { positionBrowser } from "../../ports/nativeBrowserPort";
import { useBrowserPlacement } from "./useBrowserPlacement";

vi.mock("../../ports/nativeBrowserPort", () => ({
  coverBrowser: vi.fn(() => Promise.resolve()),
  hideBrowser: vi.fn(() => Promise.resolve()),
  positionBrowser: vi.fn(() => Promise.resolve()),
}));

type Query = {
  media: string;
  listeners: Set<() => void>;
};

let queries: Query[] = [];

function resolutionQuery(media: string) {
  const query: Query = { media, listeners: new Set() };
  queries.push(query);
  return {
    media,
    addEventListener: (_type: string, listener: () => void) =>
      query.listeners.add(listener),
    removeEventListener: (_type: string, listener: () => void) =>
      query.listeners.delete(listener),
  };
}

function setDevicePixelRatio(ratio: number) {
  Object.defineProperty(window, "devicePixelRatio", {
    configurable: true,
    value: ratio,
  });
}

function placeFrame() {
  const frame = document.createElement("div");
  frame.getBoundingClientRect = () => new DOMRect(10, 20, 300, 200);
  document.body.append(frame);
  return { current: frame };
}

beforeEach(() => {
  queries = [];
  setDevicePixelRatio(1);
  vi.stubGlobal("matchMedia", vi.fn(resolutionQuery));
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
  vi.unstubAllGlobals();
  vi.mocked(positionBrowser).mockClear();
});

describe("useBrowserPlacement", () => {
  it("places the page again when the display scale changes", () => {
    const frame = placeFrame();
    renderHook(() =>
      useBrowserPlacement("browser-1", frame, { current: null }, vi.fn()),
    );
    expect(positionBrowser).toHaveBeenCalledTimes(1);
    expect(queries.map((query) => query.media)).toEqual([
      "(resolution: 1dppx)",
    ]);

    setDevicePixelRatio(2);
    for (const listener of [...queries[0].listeners]) listener();

    expect(positionBrowser).toHaveBeenCalledTimes(2);
    expect(positionBrowser).toHaveBeenLastCalledWith("browser-1", {
      x: 10,
      y: 20,
      width: 300,
      height: 200,
    });
    expect(queries.map((query) => query.media)).toEqual([
      "(resolution: 1dppx)",
      "(resolution: 2dppx)",
    ]);
    expect(queries[0].listeners.size).toBe(0);
    expect(queries[1].listeners.size).toBe(1);
  });

  it("stops watching the display scale once unmounted", () => {
    const { unmount } = renderHook(() =>
      useBrowserPlacement(
        "browser-1",
        placeFrame(),
        { current: null },
        vi.fn(),
      ),
    );
    unmount();
    expect(queries.every((query) => query.listeners.size === 0)).toBe(true);
  });
});
