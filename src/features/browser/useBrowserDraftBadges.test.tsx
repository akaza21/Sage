import { act, renderHook, waitFor } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import { useBrowserDraftBadges } from "./useBrowserDraftBadges";

const state = vi.hoisted(() => ({ contexts: [] as unknown[] }));
const invoke = vi.hoisted(() => vi.fn(() => Promise.resolve(null)));
vi.mock("../../app/agentChatOwner", () => ({
  usePinnedAgentChatOwner: () => "chat-1",
}));
vi.mock("../../app/store", () => ({
  useWorkbenchStore: (select: (value: unknown) => unknown) =>
    select({ chatUiById: { "chat-1": { attachedContext: state.contexts } } }),
}));
vi.mock("../../ports/nativeInvoke", () => ({ invokeNative: invoke }));
afterEach(() => {
  state.contexts = [];
  invoke.mockReset();
  invoke.mockResolvedValue(null);
});

const item = {
  kind: "browserElement",
  target: "area",
  ordinal: 1,
  tabId: "browser-1",
  url: "https://example.com/",
  title: "Example",
  text: "heading: Example",
  box: { x: 10, y: 20, width: 100, height: 80 },
  comment: "Simplify",
};

it("projects draft areas, clears removed items and resyncs on navigation", async () => {
  state.contexts = [item, { ...item, tabId: "browser-2" }];
  const { rerender, unmount } = renderHook(
    ({ url }) => useBrowserDraftBadges("browser-1", url),
    { initialProps: { url: item.url } },
  );
  await waitFor(() =>
    expect(invoke).toHaveBeenLastCalledWith("browser_control", {
      tabId: "browser-1",
      action: "annotateBadges",
      annotations: [item],
    }),
  );
  state.contexts = [];
  rerender({ url: item.url });
  await waitFor(() =>
    expect(invoke).toHaveBeenLastCalledWith("browser_control", {
      tabId: "browser-1",
      action: "annotateBadges",
      annotations: [],
    }),
  );
  state.contexts = [item];
  rerender({ url: "https://example.com/next" });
  await act(async () => undefined);
  expect(invoke).toHaveBeenLastCalledWith("browser_control", {
    tabId: "browser-1",
    action: "annotateBadges",
    // Rust matches the current redacted URL; the chrome URL is unredacted.
    annotations: [item],
  });
  unmount();
});

it("lets Rust match redacted context URLs against raw navigation URLs", async () => {
  const redacted = { ...item, url: "https://example.com/?token=[REDACTED]" };
  state.contexts = [redacted];
  const { unmount } = renderHook(() =>
    useBrowserDraftBadges("browser-1", "https://example.com/?token=private"),
  );
  await waitFor(() =>
    expect(invoke).toHaveBeenCalledWith("browser_control", {
      tabId: "browser-1",
      action: "annotateBadges",
      annotations: [redacted],
    }),
  );
  unmount();
});

it("coalesces pending snapshots while an IPC is in flight", async () => {
  let release!: () => void;
  invoke.mockImplementationOnce(
    () =>
      new Promise<null>((resolve) => {
        release = () => resolve(null);
      }),
  );
  const { rerender, unmount } = renderHook(() =>
    useBrowserDraftBadges("browser-1", item.url),
  );
  for (let ordinal = 1; ordinal <= 8; ordinal++) {
    state.contexts = [{ ...item, ordinal }];
    rerender();
  }
  expect(invoke).toHaveBeenCalledTimes(1);
  await act(async () => release());
  await waitFor(() => expect(invoke).toHaveBeenCalledTimes(2));
  expect(invoke.mock.calls[1]).toEqual([
    "browser_control",
    {
      tabId: "browser-1",
      action: "annotateBadges",
      annotations: [{ ...item, ordinal: 8 }],
    },
  ]);
  unmount();
});
