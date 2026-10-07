import { expect, it, vi } from "vitest";

const events = vi.hoisted(() => ({ listen: vi.fn() }));

vi.mock("@tauri-apps/api/event", () => events);
vi.mock("../ports/nativeInvoke", () => ({ isTauriRuntime: () => true }));

import { createBrowserSlice } from "./browserSlice";

it("routes an admitted native popup request through the ordinary browser open action", async () => {
  const openBrowser = vi.fn().mockResolvedValue(undefined);
  createBrowserSlice(
    vi.fn() as never,
    (() => ({ openBrowser })) as never,
    { subscribe: vi.fn() } as never,
  );
  await vi.waitFor(() =>
    expect(events.listen).toHaveBeenCalledWith(
      "browser:popup-request",
      expect.any(Function),
    ),
  );
  const listener = events.listen.mock.calls[0][1] as (event: {
    payload: string;
  }) => void;
  listener({ payload: "https://example.com/popup" });
  expect(openBrowser).toHaveBeenCalledWith("https://example.com/popup");
});
