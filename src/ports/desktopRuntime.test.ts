import { afterEach, expect, it, vi } from "vitest";
import { isTauriRuntime } from "./desktopRuntime";

afterEach(() => vi.unstubAllGlobals());

it("detects the native bridge without requiring a browser window", () => {
  vi.stubGlobal("window", undefined);
  expect(isTauriRuntime()).toBe(false);
  vi.stubGlobal("window", {});
  expect(isTauriRuntime()).toBe(false);
  vi.stubGlobal("window", { __TAURI_INTERNALS__: {} });
  expect(isTauriRuntime()).toBe(true);
  vi.stubGlobal("window", Object.create({ __TAURI_INTERNALS__: {} }));
  expect(isTauriRuntime()).toBe(true);
});
