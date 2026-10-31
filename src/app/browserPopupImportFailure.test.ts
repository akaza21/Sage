import { afterEach, expect, it, vi } from "vitest";
import { createBrowserSlice } from "./browserSlice";
const native = vi.hoisted(() => ({
  browserPopupPending: vi.fn(async () => []),
  browserPopupAbandon: vi.fn(async () => undefined),
}));
const events = vi.hoisted(() => ({
  listen: vi.fn(
    async (_name: string, _callback: (event: unknown) => void) => () =>
      undefined,
  ),
}));
vi.mock("@tauri-apps/api/event", () => events);
vi.mock("../ports/nativeInvoke", () => ({ isTauriRuntime: () => true }));
vi.mock("../ports/nativeBrowserPort", () => native);
vi.mock("./browserPopupNative", () => {
  throw new Error("chunk unavailable");
});
afterEach(() => window.dispatchEvent(new Event("beforeunload")));
it("abandons the exact hidden native child when its adoption module cannot load", async () => {
  const set = vi.fn();
  createBrowserSlice(
    set,
    (() => ({})) as never,
    { subscribe: vi.fn() } as never,
  );
  await vi.waitFor(() => expect(events.listen).toHaveBeenCalled());
  const popup = {
    id: 9,
    openerTabId: "opener",
    nativeTabId: "hidden-child",
    url: "about:blank",
  };
  const listener = events.listen.mock.calls.find(
    (call) => call[0] === "browser:popup-request",
  )?.[1] as unknown as (event: { payload: typeof popup }) => void;
  listener({ payload: popup });
  await vi.waitFor(() =>
    expect(native.browserPopupAbandon).toHaveBeenCalledWith(
      9,
      "opener",
      "hidden-child",
    ),
  );
  expect(set).toHaveBeenCalledWith({
    error: "Could not open the browser popup. Try again.",
  });
});
