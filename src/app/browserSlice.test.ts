import { expect, it, vi } from "vitest";

import { closeBrowser, openBrowser } from "../ports/nativeBrowserPort";
import { useWorkbenchStore } from "./store";

vi.mock("../ports/nativeBrowserPort", () => ({
  openBrowser: vi.fn(),
  closeBrowser: vi.fn().mockResolvedValue(undefined),
}));

it("keeps unavailable browser creation local and never exposes invocation exceptions", async () => {
  const original = useWorkbenchStore.getState();
  useWorkbenchStore.setState({
    phase: "ready",
    project: { id: "project", name: "Project", roots: [] },
    projectTransitioning: false,
    flushActiveEditor: vi.fn().mockResolvedValue(undefined),
    error: null,
  });
  try {
    const unavailable = await useWorkbenchStore.getState().openBrowser();
    expect(openBrowser).not.toHaveBeenCalled();
    expect(unavailable).toBe("Requires the desktop app.");
    expect(useWorkbenchStore.getState().error).toBeNull();
    Object.defineProperty(window, "__TAURI_INTERNALS__", {
      configurable: true,
      value: {},
    });
    vi.mocked(openBrowser).mockRejectedValueOnce(
      new TypeError("Cannot read properties of undefined (reading 'invoke')"),
    );
    const failed = await useWorkbenchStore.getState().openBrowser();
    expect(useWorkbenchStore.getState().phase).toBe("ready");
    expect(failed).toBe("Could not open the browser tab. Try again.");
    expect(useWorkbenchStore.getState().error).toBeNull();
  } finally {
    Reflect.deleteProperty(window, "__TAURI_INTERNALS__");
    useWorkbenchStore.setState(original, true);
  }
});

it("keeps positions unique after closing and recreating a browser tab", async () => {
  const original = useWorkbenchStore.getState();
  useWorkbenchStore.setState({
    project: { id: "project", name: "Project", roots: [] },
    projectTransitioning: false,
    tabs: [
      {
        id: "settings",
        kind: "settings",
        title: "Settings",
        position: 0,
        pinned: false,
        dirty: false,
      },
      {
        id: "browser-1",
        kind: "browser",
        title: "Browser",
        position: 1,
        status: "closed",
        pinned: false,
        dirty: false,
      },
      {
        id: "file-1",
        kind: "file",
        title: "File",
        position: 2,
        pinned: false,
        dirty: false,
      },
    ],
    activeTabId: "settings",
    activeEditor: null,
  });
  Object.defineProperty(window, "__TAURI_INTERNALS__", {
    configurable: true,
    value: {},
  });
  try {
    useWorkbenchStore.getState().closeTab("browser-1");
    vi.mocked(openBrowser).mockResolvedValueOnce("browser-2");
    await useWorkbenchStore.getState().openBrowser();
    expect(
      useWorkbenchStore.getState().tabs.map((tab) => tab.position),
    ).toEqual([0, 1, 2]);
  } finally {
    Reflect.deleteProperty(window, "__TAURI_INTERNALS__");
    useWorkbenchStore.setState(original);
  }
});

it("closes a restored placeholder without a native browser handle", () => {
  const original = useWorkbenchStore.getState();
  vi.mocked(closeBrowser).mockClear();
  useWorkbenchStore.setState({
    project: { id: "project", name: "Project", roots: [] },
    projectTransitioning: false,
    tabs: [
      {
        id: "browser-1",
        kind: "browser",
        title: "Browser",
        position: 0,
        pinned: false,
        dirty: false,
        status: "closed",
      },
    ],
    activeTabId: "browser-1",
  });
  useWorkbenchStore.getState().closeTab("browser-1");
  expect(useWorkbenchStore.getState().tabs).toHaveLength(0);
  expect(closeBrowser).not.toHaveBeenCalled();
  useWorkbenchStore.setState(original);
});

it("keeps browser favicon updates bounded and ordered by native revision", () => {
  const previous = useWorkbenchStore.getState();
  const tab = {
    id: "browser-favicon-projection",
    kind: "browser" as const,
    title: "Example",
    position: 0,
    pinned: false,
    dirty: false,
  };
  useWorkbenchStore.setState({ tabs: [tab] });
  const update = useWorkbenchStore.getState().updateBrowserTabFavicon;
  update(tab.id, "data:image/png;base64,iVBORw0KGgo=", "2");
  expect(useWorkbenchStore.getState().tabs[0]?.favicon).toBe(
    "data:image/png;base64,iVBORw0KGgo=",
  );
  update(tab.id, null, "1");
  expect(useWorkbenchStore.getState().tabs[0]?.favicon).toBe(
    "data:image/png;base64,iVBORw0KGgo=",
  );
  update(tab.id, "https://example.com/favicon.png", "3");
  expect(useWorkbenchStore.getState().tabs[0]?.favicon).toBeUndefined();
  useWorkbenchStore.setState({ tabs: previous.tabs });
});

it("accepts favicon revisions from a new browser after its tab ID is reused", async () => {
  const previous = useWorkbenchStore.getState();
  const tab = {
    id: "browser-reused-favicon",
    kind: "browser" as const,
    title: "Example",
    position: 0,
    pinned: false,
    dirty: false,
  };
  const update = useWorkbenchStore.getState().updateBrowserTabFavicon;
  useWorkbenchStore.setState({ tabs: [tab] });
  update(tab.id, null, "17");
  useWorkbenchStore.setState({ tabs: [] });
  await vi.waitFor(() => expect(closeBrowser).toHaveBeenCalledWith(tab.id));
  await Promise.resolve();
  useWorkbenchStore.setState({ tabs: [tab] });
  update(tab.id, "data:image/png;base64,iVBORw0KGgo=", "4");
  expect(useWorkbenchStore.getState().tabs[0]?.favicon).toBe(
    "data:image/png;base64,iVBORw0KGgo=",
  );
  useWorkbenchStore.setState({ tabs: previous.tabs });
});

it("closes an inactive native browser when its workbench tab is removed", async () => {
  const originalTabs = useWorkbenchStore.getState().tabs;
  useWorkbenchStore.setState({
    tabs: [
      ...originalTabs,
      {
        id: "browser-79",
        kind: "browser",
        title: "Browser",
        position: originalTabs.length,
        pinned: false,
        dirty: false,
      },
    ],
  });

  useWorkbenchStore.setState({ tabs: originalTabs });

  await vi.waitFor(() =>
    expect(closeBrowser).toHaveBeenCalledWith("browser-79"),
  );
});

it("keeps a browser tab visible after native close failure so Close can retry", async () => {
  vi.mocked(closeBrowser).mockReset();
  vi.mocked(closeBrowser)
    .mockRejectedValueOnce(new Error("native close failed"))
    .mockResolvedValueOnce(undefined);
  const original = useWorkbenchStore.getState();
  const tab = {
    id: "browser-close-retry",
    kind: "browser" as const,
    title: "Browser",
    position: original.tabs.length,
    pinned: false,
    dirty: false,
  };
  useWorkbenchStore.setState({
    tabs: [...original.tabs, tab],
    activeTabId: tab.id,
    error: null,
  });

  useWorkbenchStore.getState().closeTab(tab.id);
  await vi.waitFor(() =>
    expect(useWorkbenchStore.getState().error).toBe(
      "Could not close the browser tab. Try again.",
    ),
  );
  expect(useWorkbenchStore.getState().tabs).toContainEqual(tab);

  useWorkbenchStore.getState().closeTab(tab.id);
  await vi.waitFor(() =>
    expect(
      useWorkbenchStore.getState().tabs.some((item) => item.id === tab.id),
    ).toBe(false),
  );
  expect(useWorkbenchStore.getState().error).toBeNull();
  expect(closeBrowser).toHaveBeenCalledTimes(2);
  useWorkbenchStore.setState({
    tabs: original.tabs,
    activeTabId: original.activeTabId,
    error: original.error,
  });
});

it("keeps title ordering when native close fails and restores the tab", async () => {
  vi.mocked(closeBrowser)
    .mockReset()
    .mockRejectedValueOnce(new Error("close failed"))
    .mockResolvedValue(undefined);
  const original = useWorkbenchStore.getState();
  const tab = {
    id: "browser-title-close-retry",
    kind: "browser" as const,
    title: "Browser",
    position: original.tabs.length,
    pinned: false,
    dirty: false,
  };
  useWorkbenchStore.setState({ tabs: [...original.tabs, tab] });
  useWorkbenchStore
    .getState()
    .updateBrowserTabTitle(
      tab.id,
      "https://example.com/new",
      "New title",
      "12",
    );
  useWorkbenchStore.setState({ tabs: original.tabs });
  await vi.waitFor(() =>
    expect(useWorkbenchStore.getState().error).toBe(
      "Could not close the browser tab. Try again.",
    ),
  );
  expect(useWorkbenchStore.getState().tabs).toContainEqual(
    expect.objectContaining({ id: tab.id, title: "New title" }),
  );
  useWorkbenchStore
    .getState()
    .updateBrowserTabTitle(
      tab.id,
      "https://example.com/old",
      "Old title",
      "11",
    );
  expect(
    useWorkbenchStore.getState().tabs.find((item) => item.id === tab.id)?.title,
  ).toBe("New title");
  useWorkbenchStore.setState({ tabs: original.tabs, error: original.error });
});
