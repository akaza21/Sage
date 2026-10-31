import { beforeEach, expect, it, vi } from "vitest";

const events = vi.hoisted(() => ({ listen: vi.fn() }));
const native = vi.hoisted(() => ({
  browserPopupPending: vi.fn(),
  browserOpenPopup: vi.fn(),
  browserPopupAck: vi.fn(),
  browserPopupAbandon: vi.fn(),
}));

vi.mock("@tauri-apps/api/event", () => events);
vi.mock("../ports/nativeInvoke", () => ({ isTauriRuntime: () => true }));
vi.mock("../ports/nativeBrowserPort", () => native);

import { createBrowserSlice } from "./browserSlice";
import { ackPopup } from "./browserPopupNative";
import { pinAgentChatOwner, clearAgentChatOwner } from "./agentChatOwner";
import {
  acpProjectionStoreFor,
  clearAcpProjectionRegistry,
  pinAcpProjectionIds,
} from "./acpProjectionRegistry";

beforeEach(() => {
  clearAgentChatOwner();
  clearAcpProjectionRegistry();
  events.listen.mockReset();
  for (const method of Object.values(native)) method.mockReset();
});

it("replays one native popup after listener registration and places it beside its opener once", async () => {
  const popup = {
    id: 7,
    openerTabId: "browser-1",
    url: "https://example.com/popup",
  };
  native.browserPopupPending.mockResolvedValue([popup]);
  native.browserOpenPopup.mockResolvedValue({
    tabId: "browser-2",
    openerTabId: "browser-1",
    url: popup.url,
  });
  native.browserPopupAck.mockResolvedValue(undefined);
  const state = {
    project: { id: "project", roots: [] },
    projectGeneration: 1,
    projectTransitioning: false,
    tabs: [
      { id: "browser-1", kind: "browser", title: "Opener", position: 0 },
      { id: "settings", kind: "settings", title: "Settings", position: 1 },
    ],
    activeTabId: "browser-1",
    flushActiveEditor: vi.fn().mockResolvedValue(undefined),
  };
  createBrowserSlice(
    ((update: ((current: unknown) => unknown) | object) =>
      Object.assign(
        state,
        typeof update === "function" ? (update(state) as object) : update,
      )) as never,
    (() => state) as never,
    { subscribe: vi.fn() } as never,
  );
  await vi.waitFor(() =>
    expect(events.listen).toHaveBeenCalledWith(
      "browser:popup-request",
      expect.any(Function),
    ),
  );
  const listener = events.listen.mock.calls[0][1] as (event: {
    payload: typeof popup;
  }) => void;
  listener({ payload: popup });
  await vi.waitFor(() => expect(native.browserPopupAck).toHaveBeenCalled());
  expect(native.browserOpenPopup).toHaveBeenCalledTimes(1);
  expect(native.browserOpenPopup).toHaveBeenCalledWith(7, "browser-1");
  expect(native.browserPopupAck).toHaveBeenCalledWith(
    7,
    "browser-1",
    "browser-2",
  );
  expect(state.tabs.map((tab) => tab.id)).toEqual([
    "browser-1",
    "browser-2",
    "settings",
  ]);
  expect(state.tabs[1]).toMatchObject({ openerTabId: "browser-1" });
});

it("abandons the exact native popup when its Project changes before placement", async () => {
  const popup = {
    id: 8,
    openerTabId: "browser-1",
    url: "https://example.com/",
  };
  const state = {
    project: { id: "project", roots: [] },
    projectGeneration: 1,
    projectTransitioning: false,
    tabs: [{ id: "browser-1", kind: "browser", title: "Opener", position: 0 }],
    activeTabId: "browser-1",
    flushActiveEditor: vi.fn().mockResolvedValue(undefined),
    error: null as string | null,
  };
  native.browserPopupPending.mockResolvedValue([popup]);
  native.browserOpenPopup.mockImplementation(async () => {
    state.projectGeneration = 2;
    return { tabId: "browser-2", openerTabId: "browser-1", url: popup.url };
  });
  native.browserPopupAbandon.mockResolvedValue(undefined);
  createBrowserSlice(
    ((update: ((current: unknown) => unknown) | object) =>
      Object.assign(
        state,
        typeof update === "function" ? (update(state) as object) : update,
      )) as never,
    (() => state) as never,
    { subscribe: vi.fn() } as never,
  );
  await vi.waitFor(() =>
    expect(native.browserPopupAbandon).toHaveBeenCalledWith(
      8,
      "browser-1",
      "browser-2",
    ),
  );
  expect(native.browserPopupAck).not.toHaveBeenCalled();
  expect(state.tabs.map((tab) => tab.id)).toEqual(["browser-1"]);
  expect(state.error).toContain("Project changed");
});

it("replays a popup received before Project bootstrap once the Project is ready", async () => {
  const popup = {
    id: 9,
    openerTabId: "browser-1",
    url: "https://example.com/",
  };
  native.browserPopupPending.mockResolvedValue([popup]);
  native.browserOpenPopup.mockResolvedValue({
    tabId: "browser-2",
    openerTabId: "browser-1",
    url: popup.url,
  });
  native.browserPopupAck.mockResolvedValue(undefined);
  const state = {
    project: null as { id: string; roots: [] } | null,
    projectGeneration: 1,
    projectTransitioning: true,
    tabs: [{ id: "browser-1", kind: "browser", title: "Opener", position: 0 }],
    activeTabId: "browser-1",
    flushActiveEditor: vi.fn().mockResolvedValue(undefined),
    error: null as string | null,
  };
  const subscribe = vi.fn();
  createBrowserSlice(
    ((update: ((current: unknown) => unknown) | object) =>
      Object.assign(
        state,
        typeof update === "function" ? (update(state) as object) : update,
      )) as never,
    (() => state) as never,
    { subscribe } as never,
  );
  await vi.waitFor(() => expect(native.browserPopupPending).toHaveBeenCalled());
  expect(native.browserOpenPopup).not.toHaveBeenCalled();
  const previous = { ...state };
  state.project = { id: "project", roots: [] };
  state.projectTransitioning = false;
  const changed = subscribe.mock.calls[0][0] as (
    current: unknown,
    previous: unknown,
  ) => void;
  changed(state, previous);
  await vi.waitFor(() =>
    expect(native.browserPopupAck).toHaveBeenCalledWith(
      9,
      "browser-1",
      "browser-2",
    ),
  );
  expect(state.tabs.map((tab) => tab.id)).toEqual(["browser-1", "browser-2"]);
});

it("replays a completion event that races with an in-progress popup response", async () => {
  const popup = {
    id: 10,
    openerTabId: "browser-1",
    url: "https://example.com/",
  };
  const opened = {
    tabId: "browser-2",
    openerTabId: "browser-1",
    url: popup.url,
  };
  let rejectFirst: ((reason?: unknown) => void) | undefined;
  native.browserPopupPending.mockResolvedValue([popup]);
  native.browserOpenPopup
    .mockImplementationOnce(
      () =>
        new Promise((_, reject) => {
          rejectFirst = reject;
        }),
    )
    .mockResolvedValue(opened);
  native.browserPopupAck.mockResolvedValue(undefined);
  const state = {
    project: { id: "project", roots: [] },
    projectGeneration: 1,
    projectTransitioning: false,
    tabs: [{ id: "browser-1", kind: "browser", title: "Opener", position: 0 }],
    activeTabId: "browser-1",
    flushActiveEditor: vi.fn().mockResolvedValue(undefined),
    error: null as string | null,
  };
  createBrowserSlice(
    ((update: ((current: unknown) => unknown) | object) =>
      Object.assign(
        state,
        typeof update === "function" ? (update(state) as object) : update,
      )) as never,
    (() => state) as never,
    { subscribe: vi.fn() } as never,
  );
  await vi.waitFor(() =>
    expect(native.browserOpenPopup).toHaveBeenCalledTimes(1),
  );
  const listener = events.listen.mock.calls[0][1] as (event: {
    payload: typeof popup;
  }) => void;
  listener({ payload: popup });
  rejectFirst?.(new Error("popup open is in progress"));
  await vi.waitFor(() =>
    expect(native.browserPopupAck).toHaveBeenCalledWith(
      10,
      "browser-1",
      "browser-2",
    ),
  );
  expect(native.browserOpenPopup).toHaveBeenCalledTimes(2);
});

it("abandons a native opener-linked child when no project can adopt it", async () => {
  const popup = {
    id: 9,
    openerTabId: "browser-1",
    url: "about:blank",
    nativeTabId: "browser-4",
  };
  native.browserPopupPending.mockResolvedValue([]);
  native.browserPopupAbandon.mockResolvedValue(undefined);
  const state = {
    project: { id: "project", roots: [] },
    projectGeneration: 1,
    projectTransitioning: true,
    tabs: [{ id: "browser-1", kind: "browser", title: "Opener", position: 0 }],
    activeTabId: "browser-1",
    flushActiveEditor: vi.fn().mockResolvedValue(undefined),
  };
  createBrowserSlice(
    ((update: ((current: unknown) => unknown) | object) =>
      Object.assign(
        state,
        typeof update === "function" ? (update(state) as object) : update,
      )) as never,
    (() => state) as never,
    { subscribe: vi.fn() } as never,
  );
  await vi.waitFor(() =>
    expect(events.listen).toHaveBeenCalledWith(
      "browser:popup-request",
      expect.any(Function),
    ),
  );
  const listener = events.listen.mock.calls[0][1] as (event: {
    payload: Partial<typeof popup>;
  }) => void;
  listener({ payload: popup });
  await vi.waitFor(() =>
    expect(native.browserPopupAbandon).toHaveBeenCalledWith(
      9,
      "browser-1",
      "browser-4",
    ),
  );
  listener({ payload: { ...popup, id: 10, nativeTabId: undefined } });
  await new Promise((resolve) => setTimeout(resolve, 0));
  expect(native.browserPopupAbandon).toHaveBeenCalledTimes(1);
  expect(native.browserOpenPopup).not.toHaveBeenCalled();
  expect(state.tabs).toHaveLength(1);
});

function mountWithOpener(overrides: object = {}) {
  const state = {
    project: { id: "project", roots: [] },
    projectGeneration: 1,
    projectTransitioning: false,
    tabs: [{ id: "browser-1", kind: "browser", title: "Opener", position: 0 }],
    activeTabId: "browser-1",
    flushActiveEditor: vi.fn().mockResolvedValue(undefined),
    ...overrides,
  };
  createBrowserSlice(
    ((update: ((current: unknown) => unknown) | object) =>
      Object.assign(
        state,
        typeof update === "function" ? (update(state) as object) : update,
      )) as never,
    (() => state) as never,
    { subscribe: vi.fn() } as never,
  );
  return state;
}

async function popupListener() {
  await vi.waitFor(() =>
    expect(events.listen).toHaveBeenCalledWith(
      "browser:popup-request",
      expect.any(Function),
    ),
  );
  return events.listen.mock.calls[0][1] as (event: { payload: object }) => void;
}

it("removes a just-inserted popup tab when its late ack fails", async () => {
  native.browserPopupPending.mockResolvedValue([]);
  native.browserOpenPopup.mockResolvedValue({
    tabId: "browser-4",
    openerTabId: "browser-1",
    url: "https://example.com/",
  });
  native.browserPopupAck.mockRejectedValue(new Error("popup tab unavailable"));
  native.browserPopupAbandon.mockResolvedValue(undefined);
  const state = mountWithOpener();
  const listener = await popupListener();
  listener({
    payload: {
      id: 11,
      openerTabId: "browser-1",
      url: "https://example.com/",
      nativeTabId: "browser-4",
    },
  });
  await vi.waitFor(() =>
    expect(state).toMatchObject({
      error: "Couldn't open popup: popup tab unavailable",
    }),
  );
  expect(native.browserPopupAck).toHaveBeenCalledWith(
    11,
    "browser-1",
    "browser-4",
  );
  expect(state.tabs.map((tab) => tab.id)).toEqual(["browser-1"]);
  expect(state.activeTabId).toBe("browser-1");
  expect(native.browserPopupAbandon).toHaveBeenCalledWith(
    11,
    "browser-1",
    "browser-4",
  );
});

it("does not reactivate the mounted chat after a popup ack fails", async () => {
  native.browserPopupAck.mockRejectedValue(
    new Error("popup acknowledgement unavailable"),
  );
  native.browserPopupAbandon.mockResolvedValue(undefined);
  const activateChat = vi.fn();
  pinAgentChatOwner("chat-1");
  const state = mountWithOpener({
    tabs: [
      { id: "chat-1", kind: "chat", title: "Chat", position: 0 },
      { id: "browser-4", kind: "browser", title: "Popup", position: 1 },
    ],
    activeTabId: "browser-4",
    activateChat,
  });
  await expect(
    ackPopup(
      ((update: object) => Object.assign(state, update)) as never,
      (() => state) as never,
      21,
      {
        tabId: "browser-4",
        openerTabId: "browser-1",
        url: "https://example.com/",
      },
      true,
    ),
  ).rejects.toThrow("popup acknowledgement unavailable");
  expect(activateChat).not.toHaveBeenCalled();
  expect(state.activeTabId).toBe("chat-1");
  expect(state.tabs.map((tab) => tab.id)).toEqual(["chat-1"]);
  clearAgentChatOwner();
});

it("keeps both mounted projections when failed adoption selects the unfocused chat", async () => {
  pinAgentChatOwner("chat-a");
  pinAcpProjectionIds(["chat-a", "chat-b"]);
  const first = acpProjectionStoreFor("chat-a");
  const second = acpProjectionStoreFor("chat-b");
  for (const [chatId, store] of [
    ["chat-a", first],
    ["chat-b", second],
  ] as const) {
    store.getState().activateScope({
      projectId: "project",
      projectGeneration: "1",
      chatId,
    } as never);
  }
  const firstState = first.getState();
  const secondState = second.getState();
  const firstChanged = vi.fn();
  const secondChanged = vi.fn();
  const unsubscribeFirst = first.subscribe(firstChanged);
  const unsubscribeSecond = second.subscribe(secondChanged);
  const activateChat = vi.fn(() => {
    first.getState().clear();
    second.getState().clear();
  });
  native.browserPopupAck.mockRejectedValue(
    new Error("popup acknowledgement unavailable"),
  );
  native.browserPopupAbandon.mockResolvedValue(undefined);
  const state = mountWithOpener({
    tabs: [
      { id: "chat-a", kind: "chat", title: "A", position: 0 },
      { id: "browser-1", kind: "browser", title: "Opener", position: 1 },
      { id: "browser-4", kind: "browser", title: "Popup", position: 2 },
      { id: "chat-b", kind: "chat", title: "B", position: 3 },
    ],
    activeTabId: "browser-4",
    activateChat,
  });
  try {
    await expect(
      ackPopup(
        ((update: object) => Object.assign(state, update)) as never,
        (() => state) as never,
        23,
        {
          tabId: "browser-4",
          openerTabId: "browser-1",
          url: "https://example.com/",
        },
        true,
      ),
    ).rejects.toThrow("popup acknowledgement unavailable");
    expect(activateChat).not.toHaveBeenCalled();
    expect(state.activeTabId).toBe("chat-b");
    expect(state.projectGeneration).toBe(1);
    expect(state.projectTransitioning).toBe(false);
    expect(first.getState()).toBe(firstState);
    expect(second.getState()).toBe(secondState);
    expect(firstChanged).not.toHaveBeenCalled();
    expect(secondChanged).not.toHaveBeenCalled();
  } finally {
    unsubscribeFirst();
    unsubscribeSecond();
    clearAcpProjectionRegistry();
    clearAgentChatOwner();
  }
});

it("retains a plain popup tab as cleanup owner when native close is unresolved", async () => {
  native.browserPopupPending.mockResolvedValue([]);
  native.browserOpenPopup.mockResolvedValue({
    tabId: "browser-4",
    openerTabId: "browser-1",
    url: "https://example.com/",
  });
  native.browserPopupAck.mockRejectedValue(
    new Error("popup acknowledgement unavailable"),
  );
  native.browserPopupAbandon.mockRejectedValue(
    new Error("popup native close is unresolved"),
  );
  const state = mountWithOpener();
  const listener = await popupListener();
  listener({
    payload: {
      id: 22,
      openerTabId: "browser-1",
      nativeTabId: "browser-4",
      url: "https://example.com/",
    },
  });
  await vi.waitFor(() =>
    expect(state).toMatchObject({
      error: "Couldn't open popup: popup native close is unresolved",
    }),
  );
  expect(state.tabs.map((tab) => tab.id)).toEqual(["browser-1", "browser-4"]);
});

it("abandons the native child when flushing fails before opening", async () => {
  native.browserPopupPending.mockResolvedValue([]);
  native.browserPopupAbandon.mockResolvedValue(undefined);
  const state = mountWithOpener();
  state.flushActiveEditor.mockRejectedValue(new Error("flush failed"));
  const listener = await popupListener();
  listener({
    payload: {
      id: 23,
      openerTabId: "browser-1",
      nativeTabId: "browser-4",
      url: "https://example.com/",
    },
  });
  await vi.waitFor(() =>
    expect(native.browserPopupAbandon).toHaveBeenCalledWith(
      23,
      "browser-1",
      "browser-4",
    ),
  );
  expect(native.browserOpenPopup).not.toHaveBeenCalled();
});

it("abandons a native child when the Project changes during the editor flush", async () => {
  native.browserPopupPending.mockResolvedValue([]);
  native.browserPopupAbandon.mockResolvedValue(undefined);
  const state = mountWithOpener();
  state.flushActiveEditor.mockImplementation(async () => {
    state.projectGeneration = 2;
  });
  const listener = await popupListener();
  listener({
    payload: {
      id: 12,
      openerTabId: "browser-1",
      url: "about:blank",
      nativeTabId: "browser-5",
    },
  });
  await vi.waitFor(() =>
    expect(native.browserPopupAbandon).toHaveBeenCalledWith(
      12,
      "browser-1",
      "browser-5",
    ),
  );
  expect(native.browserOpenPopup).not.toHaveBeenCalled();
});
