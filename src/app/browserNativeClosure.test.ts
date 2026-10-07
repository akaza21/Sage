import { afterEach, beforeEach, expect, it, vi } from "vitest";
import type * as NativeInvoke from "../ports/nativeInvoke";

const events = vi.hoisted(() => ({ listen: vi.fn() }));

vi.mock("@tauri-apps/api/event", () => events);
vi.mock("../ports/nativeInvoke", async (importOriginal) => ({
  ...(await importOriginal<typeof NativeInvoke>()),
  isTauriRuntime: () => true,
}));
vi.mock("../ports/nativeBrowserPort", () => ({
  openBrowser: vi.fn(),
  closeBrowser: vi.fn().mockResolvedValue(undefined),
}));

import { closeBrowser } from "../ports/nativeBrowserPort";
import { createBrowserSlice } from "./browserSlice";
import { useWorkbenchStore } from "./store";

const original = useWorkbenchStore.getState();
const fileResource = { rootId: "root", relativePath: "file.txt" };
const fileTab = {
  id: "file-1",
  kind: "file" as const,
  title: "file.txt",
  position: 0,
  pinned: false,
  dirty: false,
  resource: fileResource,
};
const browserTab = {
  id: "browser-native-closed",
  kind: "browser" as const,
  title: "Browser",
  position: 1,
  pinned: false,
  dirty: false,
};

async function nativeClosedListener() {
  createBrowserSlice(useWorkbenchStore.setState, useWorkbenchStore.getState, {
    subscribe: vi.fn(),
  } as never);
  await vi.waitFor(() =>
    expect(events.listen).toHaveBeenCalledWith(
      "browser:tab-closed",
      expect.any(Function),
    ),
  );
  return events.listen.mock.calls.find(
    ([name]) => name === "browser:tab-closed",
  )?.[1] as (event: { payload: string }) => void;
}

async function agentOpenedListener() {
  createBrowserSlice(useWorkbenchStore.setState, useWorkbenchStore.getState, {
    subscribe: vi.fn(),
  } as never);
  await vi.waitFor(() =>
    expect(events.listen).toHaveBeenCalledWith(
      "browser:agent-opened",
      expect.any(Function),
    ),
  );
  return events.listen.mock.calls.find(
    ([name]) => name === "browser:agent-opened",
  )?.[1] as (event: {
    payload: { projectId: string; tabId: string; url: string };
  }) => void;
}

async function navigationListener() {
  createBrowserSlice(useWorkbenchStore.setState, useWorkbenchStore.getState, {
    subscribe: vi.fn(),
  } as never);
  await vi.waitFor(() =>
    expect(events.listen).toHaveBeenCalledWith(
      "browser:navigation",
      expect.any(Function),
    ),
  );
  return events.listen.mock.calls.find(
    ([name]) => name === "browser:navigation",
  )?.[1] as (event: {
    payload: {
      tabId: string;
      url: string;
      title: string | null;
      revision: string;
      loading: boolean;
      error: string | null;
    };
  }) => void;
}

beforeEach(() => {
  events.listen.mockReset().mockResolvedValue(vi.fn());
  vi.mocked(closeBrowser).mockClear();
  useWorkbenchStore.setState({
    tabs: [fileTab, browserTab],
    activeTabId: fileTab.id,
    activeEditor: { phase: "loading", resource: fileResource },
    projectTransitioning: false,
  });
  vi.mocked(closeBrowser).mockClear();
});

afterEach(() => {
  useWorkbenchStore.setState({
    tabs: original.tabs,
    activeTabId: original.activeTabId,
    activeEditor: original.activeEditor,
    projectTransitioning: original.projectTransitioning,
    project: original.project,
    flushActiveEditor: original.flushActiveEditor,
  });
});

it("removes the native-closed browser without closing it twice or clearing the active file editor", async () => {
  const listener = await nativeClosedListener();
  listener({ payload: browserTab.id });

  expect(useWorkbenchStore.getState().tabs).toEqual([fileTab]);
  expect(useWorkbenchStore.getState().activeEditor).toEqual({
    phase: "loading",
    resource: fileResource,
  });
  expect(closeBrowser).not.toHaveBeenCalled();
});

it("updates an inactive browser tab from a native page title event", async () => {
  const listener = await navigationListener();
  listener({
    payload: {
      tabId: browserTab.id,
      url: "https://example.com/page",
      title: "Example Domain",
      revision: "1",
      loading: false,
      error: null,
    },
  });
  expect(
    useWorkbenchStore.getState().tabs.find((tab) => tab.id === browserTab.id)
      ?.title,
  ).toBe("Example Domain");
  listener({
    payload: {
      tabId: browserTab.id,
      url: "https://example.com/next",
      title: "Late title",
      revision: "2",
      loading: true,
      error: null,
    },
  });
  expect(
    useWorkbenchStore.getState().tabs.find((tab) => tab.id === browserTab.id)
      ?.title,
  ).toBe("Example Domain");
});

it("strips bidi controls and ignores an older native title event", async () => {
  const listener = await navigationListener();
  listener({
    payload: {
      tabId: browserTab.id,
      url: "https://example.com/new",
      title: "New\u061c\u202e spoof\u202c Title",
      revision: "12",
      loading: false,
      error: null,
    },
  });
  listener({
    payload: {
      tabId: browserTab.id,
      url: "https://example.com/old",
      title: "Old title",
      revision: "11",
      loading: false,
      error: null,
    },
  });
  expect(
    useWorkbenchStore.getState().tabs.find((tab) => tab.id === browserTab.id)
      ?.title,
  ).toBe("New spoof Title");
});

it("ignores a native closure event after the user has already closed that browser tab", async () => {
  const listener = await nativeClosedListener();
  useWorkbenchStore.getState().closeTab(browserTab.id);
  await vi.waitFor(() =>
    expect(useWorkbenchStore.getState().tabs).toEqual([fileTab]),
  );
  listener({ payload: browserTab.id });

  expect(closeBrowser).toHaveBeenCalledTimes(1);
  expect(useWorkbenchStore.getState().tabs).toEqual([fileTab]);
});

it("shows an agent-opened browser in the current project's workbench", async () => {
  useWorkbenchStore.setState({
    project: { id: "project-1", name: "Project", roots: [] },
  });
  const listener = await agentOpenedListener();
  listener({
    payload: {
      projectId: "project-1",
      tabId: "browser-77",
      url: "https://example.com/page",
    },
  });
  await vi.waitFor(() =>
    expect(useWorkbenchStore.getState().activeTabId).toBe("browser-77"),
  );
  const state = useWorkbenchStore.getState();
  expect(state.tabs.find((tab) => tab.id === "browser-77")).toMatchObject({
    kind: "browser",
    title: "example.com",
  });
  expect(state.activeTabId).toBe("browser-77");
  await vi.waitFor(() =>
    expect(useWorkbenchStore.getState().activeEditor).toBeNull(),
  );
  expect(closeBrowser).not.toHaveBeenCalled();
});

it("ignores an agent-opened browser event for another project", async () => {
  useWorkbenchStore.setState({
    project: { id: "project-1", name: "Project", roots: [] },
  });
  const listener = await agentOpenedListener();
  listener({
    payload: {
      projectId: "project-2",
      tabId: "browser-78",
      url: "https://example.com/",
    },
  });
  expect(
    useWorkbenchStore.getState().tabs.some((tab) => tab.id === "browser-78"),
  ).toBe(false);
});

it("keeps an agent-opened tab visible and closeable when editor flush fails", async () => {
  useWorkbenchStore.setState({
    project: { id: "project-1", name: "Project", roots: [] },
    flushActiveEditor: vi.fn().mockRejectedValue(new Error("flush failed")),
  });
  const listener = await agentOpenedListener();
  listener({
    payload: {
      projectId: "project-1",
      tabId: "browser-79",
      url: "https://example.com/",
    },
  });
  await vi.waitFor(() =>
    expect(useWorkbenchStore.getState().error).toContain("flush failed"),
  );
  expect(useWorkbenchStore.getState().tabs).toContainEqual(
    expect.objectContaining({ id: "browser-79", kind: "browser" }),
  );
  expect(useWorkbenchStore.getState().activeTabId).toBe("browser-79");
  expect(useWorkbenchStore.getState().activeEditor).toEqual({
    phase: "loading",
    resource: fileResource,
  });
  useWorkbenchStore.getState().closeTab("browser-79");
  await vi.waitFor(() =>
    expect(closeBrowser).toHaveBeenCalledWith("browser-79"),
  );
});
