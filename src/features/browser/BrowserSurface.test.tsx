import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { listen } from "@tauri-apps/api/event";
import { Activity } from "react";

import type { WorkbenchTab } from "../../domain/workbench";
import { BrowserSurface } from "./BrowserSurface";
import { browserOperationFixture } from "../../domain/browserOperation.fixture";
import { recordBrowserAgentOperation } from "../../app/browserAgentActivityStore";
import { useWorkbenchStore } from "../../app/store";
import { pinAgentChatOwner } from "../../app/agentChatOwner";
import { useAgentSettingsState } from "../settings/agentSettingsState";
// The new-tab page, with its New page button, loads lazily. Load it with the
// file, so its first render does not race a cold import against a one-second
// query.
import "./BrowserNewTab";

const browserStatus = vi.fn();
const openBrowser = vi.fn(async (_url?: string) => "browser-new");
const browserPageContext = vi.fn();
const browserZoom = vi.fn(
  async (_tabId: string, _percent: number) => undefined,
);
const browserFind = vi.fn(
  async (_tabId: string, _action: string, _query?: string) => undefined,
);
const browserOpenExternal = vi.fn(async (_tabId: string) => undefined);
const browserNavigate = vi.fn(
  async (_tabId: string, _url: string) => undefined,
);
const hideBrowser = vi.fn(async (_tabId: string) => undefined);
const browserControl = vi.fn(
  async (_tabId: string, _action: string) => undefined,
);
const coverBrowser = vi.fn(async (_tabId: string) => undefined);
const positionBrowser = vi.fn(
  async (_tabId: string, _bounds: unknown) => undefined,
);
const browserSuggestedSites = vi.fn(async () => ["https://example.com/"]);
const browserHistoryPage = vi.fn(async (_query: unknown) => ({
  visits: [
    {
      id: 1,
      url: "https://history.example/page",
      title: "History Example",
      visitedAt: Date.now(),
    },
  ],
  next: null,
}));
const attachBrowserTab = vi.fn(
  async (_projectId: string, _chatId: string, _tabId: string) =>
    ({}) as unknown,
);
const detachBrowserTab = vi.fn(
  async (_projectId: string, _chatId: string) => undefined,
);
/** The tab's Agent attachment as Rust holds it. */
let rustAgentControl: {
  projectId: string;
  chatId: string;
  userControl: boolean;
  developerMode: boolean;
} | null = null;
const stopBrowserTab = vi.fn(async (_tabId: string) => {
  rustAgentControl = null;
});
const browserAgentControl = vi.fn(async (_tabId: string, action: string) => {
  if (rustAgentControl && action !== "agentControl")
    rustAgentControl = {
      ...rustAgentControl,
      userControl: action === "agentTakeOver",
    };
  return rustAgentControl && { ...rustAgentControl };
});

vi.mock("../../ports/nativeInvoke", () => ({ isTauriRuntime: () => true }));
vi.mock("../../ports/nativeBrowserPort", () => ({
  openBrowser: (url?: string) => openBrowser(url),
  closeBrowser: vi.fn(async () => undefined),
  browserPopupPending: vi.fn(async () => []),
  browserStatus: (tabId: string) => browserStatus(tabId),
  browserPageContext: (tabId: string, revision: string) =>
    browserPageContext(tabId, revision),
  browserZoom: (tabId: string, percent: number) => browserZoom(tabId, percent),
  browserFind: (tabId: string, action: string, query?: string) =>
    browserFind(tabId, action, query),
  browserOpenExternal: (tabId: string) => browserOpenExternal(tabId),
  attachBrowserTab: (projectId: string, chatId: string, tabId: string) =>
    attachBrowserTab(projectId, chatId, tabId),
  detachBrowserTab: (projectId: string, chatId: string) =>
    detachBrowserTab(projectId, chatId),
  stopBrowserTab: (tabId: string) => stopBrowserTab(tabId),
  browserAttachmentStatus: vi.fn(async () => null),
  browserPermissionStatus: vi.fn(async (tabId: string) => ({
    tabId,
    origin: "unavailable",
    pending: [],
    decisions: [],
  })),
  browserControl: (tabId: string, action: string) =>
    browserControl(tabId, action),
  browserAgentControl: (tabId: string, action: string) =>
    browserAgentControl(tabId, action),
  browserNavigate: (tabId: string, url: string) => browserNavigate(tabId, url),
  hideBrowser: (tabId: string) => hideBrowser(tabId),
  coverBrowser: (tabId: string) => coverBrowser(tabId),
  positionBrowser: (tabId: string, bounds: unknown) =>
    positionBrowser(tabId, bounds),
}));
vi.mock("../../ports/nativeBrowserHistoryPort", () => ({
  browserSuggestedSites: () => browserSuggestedSites(),

  browserHistoryPage: (query: unknown) => browserHistoryPage(query),
  browserHistoryRemove: vi.fn(async () => 1),
}));
// Automatic mocks also cover concurrent demand imports of the event API.
vi.mock("@tauri-apps/api/event");

const tab: WorkbenchTab = {
  id: "browser-1",
  kind: "browser",
  title: "Example",
  position: 0,
  pinned: false,
  dirty: false,
};

it("shows a closed placeholder for a restored browser without contacting native state", async () => {
  render(<BrowserSurface tab={{ ...tab, status: "closed" }} />);
  expect(
    await screen.findByText(/browser session closed when Sage restarted/i),
  ).toBeInTheDocument();
  expect(
    screen.getByRole("button", { name: "Open new browser" }),
  ).toBeInTheDocument();
  expect(browserStatus).not.toHaveBeenCalled();
});

it("shows a restored page creation failure locally and clears it on the next attempt", async () => {
  const previous = useWorkbenchStore.getState();
  useWorkbenchStore.setState({
    openBrowser: async () => "Could not open the browser tab. Try again.",
    error: "Existing composer feedback",
  });
  try {
    render(<BrowserSurface tab={{ ...tab, status: "closed" }} />);
    const open = await screen.findByRole("button", {
      name: "Open new browser",
    });
    fireEvent.click(open);
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "Could not open the browser tab. Try again.",
    );
    expect(useWorkbenchStore.getState().error).toBe(
      "Existing composer feedback",
    );
    act(() =>
      useWorkbenchStore.setState({ openBrowser: async () => undefined }),
    );
    fireEvent.click(open);
    await waitFor(() => expect(screen.queryByRole("alert")).toBeNull());
  } finally {
    useWorkbenchStore.setState(previous, true);
  }
});

it("shows a New page creation failure in the browser without changing composer feedback", async () => {
  const previous = useWorkbenchStore.getState();
  browserStatus.mockResolvedValueOnce({
    tabId: tab.id,
    url: "",
    revision: "1",
    loading: false,
  });
  useWorkbenchStore.setState({
    openBrowser: async () => "Browser tab limit reached.",
    error: "Existing composer feedback",
  });
  try {
    render(<BrowserSurface tab={tab} />);
    fireEvent.click(await screen.findByRole("button", { name: "New page" }));
    expect(await screen.findByText("Browser tab limit reached.")).toBeVisible();
    expect(useWorkbenchStore.getState().error).toBe(
      "Existing composer feedback",
    );
  } finally {
    useWorkbenchStore.setState(previous, true);
  }
});

beforeEach(() => {
  vi.clearAllMocks();
  vi.mocked(listen).mockResolvedValue(() => undefined);
  rustAgentControl = null;
  vi.stubGlobal(
    "ResizeObserver",
    class {
      observe() {}
      disconnect() {}
    },
  );
  browserStatus.mockResolvedValue({
    tabId: tab.id,
    url: "https://example.com/path",
    revision: "1",
    loading: false,
    canGoBack: false,
    canGoForward: false,
    zoomPercent: 100,
  });
});

afterEach(() => {
  cleanup();
  useAgentSettingsState.getState().close();
  pinAgentChatOwner(null);
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
});

it("routes Browser settings to Agent settings with the originating tab", async () => {
  render(<BrowserSurface tab={tab} />);
  await waitFor(() => expect(browserStatus).toHaveBeenCalledWith(tab.id));
  fireEvent.click(screen.getByRole("button", { name: "More options" }));
  fireEvent.click(screen.getByRole("menuitem", { name: "Browser settings" }));
  expect(useAgentSettingsState.getState().page).toBe("Browser");
  expect(useAgentSettingsState.getState().browserTabId).toBe(tab.id);
  expect(
    screen.queryByRole("complementary", { name: "Browser settings" }),
  ).toBeNull();
});

it("routes Site settings to the originating tab with permissions expanded", async () => {
  render(<BrowserSurface tab={tab} />);
  await waitFor(() => expect(browserStatus).toHaveBeenCalledWith(tab.id));
  fireEvent.click(screen.getByRole("button", { name: "More options" }));
  fireEvent.click(screen.getByRole("menuitem", { name: "Site settings" }));
  expect(useAgentSettingsState.getState()).toMatchObject({
    page: "Browser",
    browserTabId: tab.id,
    showSitePermissions: true,
  });
});

it("disposes a navigation subscription that resolves after Activity sleeps without bootstrapping hidden browser state", async () => {
  let finish: ((dispose: () => void) => void) | undefined;
  const lateDispose = vi.fn();
  vi.mocked(listen).mockImplementation(async (name) =>
    name === "browser:navigation"
      ? new Promise<() => void>((resolve) => {
          finish = resolve;
        })
      : () => undefined,
  );
  const view = render(
    <Activity mode="visible">
      <BrowserSurface tab={tab} />
    </Activity>,
  );
  await waitFor(() => expect(finish).toBeDefined());
  view.rerender(
    <Activity mode="hidden">
      <BrowserSurface tab={tab} />
    </Activity>,
  );
  await act(async () => finish?.(lateDispose));
  expect(lateDispose).toHaveBeenCalledOnce();
  expect(browserStatus).not.toHaveBeenCalled();
  view.unmount();
  vi.mocked(listen).mockResolvedValue(() => undefined);
});

it("shows a native favicon after an earlier navigation event arrives during status hydration", async () => {
  const previous = useWorkbenchStore.getState();
  const favicon = "data:image/png;base64,iVBORw0KGgo=";
  let deliverNavigation: ((event: { payload: unknown }) => void) | undefined;
  let resolveStatus!: (value: unknown) => void;
  browserStatus.mockReturnValueOnce(
    new Promise((resolve) => {
      resolveStatus = resolve;
    }),
  );
  vi.mocked(listen).mockImplementation(async (name, callback) => {
    if (name === "browser:navigation")
      deliverNavigation = callback as typeof deliverNavigation;
    return () => undefined;
  });
  useWorkbenchStore.setState({ tabs: [tab] });
  render(<BrowserSurface tab={tab} />);
  await waitFor(() => expect(browserStatus).toHaveBeenCalledWith(tab.id));
  await waitFor(() => expect(deliverNavigation).toBeDefined());
  act(() => {
    deliverNavigation?.({
      payload: {
        tabId: tab.id,
        url: "https://example.com/path",
        title: "Example",
        favicon: null,
        revision: "4",
        loading: false,
        canGoBack: false,
        canGoForward: false,
        zoomPercent: 100,
      },
    });
    resolveStatus({
      tabId: tab.id,
      url: "https://example.com/path",
      title: "Example",
      favicon,
      revision: "5",
      loading: false,
      canGoBack: false,
      canGoForward: false,
      zoomPercent: 100,
    });
  });
  await waitFor(() =>
    expect(useWorkbenchStore.getState().tabs[0]?.favicon).toBe(favicon),
  );
  useWorkbenchStore.setState({ tabs: previous.tabs });
});

it("shows a favicon that arrives after the initial browser status", async () => {
  const previous = useWorkbenchStore.getState();
  const favicon = "data:image/png;base64,iVBORw0KGgo=";
  let deliverNavigation: ((event: { payload: unknown }) => void) | undefined;
  vi.mocked(listen).mockImplementation(async (name, callback) => {
    if (name === "browser:navigation")
      deliverNavigation = callback as typeof deliverNavigation;
    return () => undefined;
  });
  useWorkbenchStore.setState({ tabs: [tab] });
  render(<BrowserSurface tab={tab} />);
  await waitFor(() => expect(browserStatus).toHaveBeenCalledWith(tab.id));
  await waitFor(() => expect(deliverNavigation).toBeDefined());
  act(() => {
    deliverNavigation?.({
      payload: {
        tabId: tab.id,
        url: "https://example.com/path",
        title: "Example",
        favicon,
        revision: "2",
        loading: false,
        canGoBack: false,
        canGoForward: false,
        zoomPercent: 100,
      },
    });
  });
  await waitFor(() =>
    expect(useWorkbenchStore.getState().tabs[0]?.favicon).toBe(favicon),
  );
  useWorkbenchStore.setState({ tabs: previous.tabs });
});

it("attaches a captured page to the focused Chat only after the explicit click", async () => {
  const previous = useWorkbenchStore.getState();
  const chat = {
    ...tab,
    id: "chat-ask",
    kind: "chat" as const,
    title: "Research Chat",
  };
  useWorkbenchStore.setState({
    project: { id: "project-ask" } as typeof previous.project,
    tabs: [...previous.tabs, chat],
    activeTabId: tab.id,
  });
  pinAgentChatOwner(chat.id);
  browserPageContext.mockResolvedValue({
    kind: "browserPage",
    tabId: tab.id,
    url: "https://example.com/path",
    title: "Example page",
    text: "Page accessibility text:\nAn article",
  });
  render(<BrowserSurface tab={tab} />);
  const ask = await screen.findByRole("button", {
    name: "Ask about this page",
  });
  expect(
    useWorkbenchStore.getState().chatUiById[chat.id]?.attachedContext ?? [],
  ).toEqual([]);
  fireEvent.click(ask);
  await waitFor(() =>
    expect(
      useWorkbenchStore.getState().chatUiById[chat.id].attachedContext,
    ).toEqual([
      {
        kind: "browserPage",
        tabId: tab.id,
        url: "https://example.com/path",
        title: "Example page",
        text: "Page accessibility text:\nAn article",
      },
    ]),
  );
  useWorkbenchStore.setState({
    project: previous.project,
    tabs: previous.tabs,
    activeTabId: previous.activeTabId,
    chatUiById: previous.chatUiById,
  });
});

it("refuses Ask about this page when no Chat is focused", async () => {
  pinAgentChatOwner(null);
  render(<BrowserSurface tab={tab} />);
  expect(
    await screen.findByRole("button", { name: "Ask about this page" }),
  ).toBeDisabled();
  expect(browserPageContext).not.toHaveBeenCalled();
});

it("rejects page capture if navigation changes before attachment", async () => {
  const previous = useWorkbenchStore.getState();
  const chat = { ...tab, id: "chat-stale", kind: "chat" as const };
  useWorkbenchStore.setState({
    project: { id: "project-stale" } as typeof previous.project,
    tabs: [...previous.tabs, chat],
    activeTabId: tab.id,
  });
  pinAgentChatOwner(chat.id);
  browserPageContext.mockResolvedValue({
    kind: "browserPage",
    tabId: tab.id,
    url: "https://example.com/path",
    revision: "1",
    title: "Old page",
    text: "Old text",
  });
  browserStatus.mockResolvedValueOnce({
    tabId: tab.id,
    url: "https://example.com/path",
    revision: "1",
    loading: false,
    canGoBack: false,
    canGoForward: false,
    zoomPercent: 100,
  });
  browserStatus.mockResolvedValueOnce({
    tabId: tab.id,
    url: "https://example.com/path",
    loading: false,
    canGoBack: false,
    canGoForward: false,
    zoomPercent: 100,
  });
  browserStatus.mockResolvedValueOnce({
    tabId: tab.id,
    url: "https://example.com/next",
    revision: "2",
    loading: false,
    canGoBack: false,
    canGoForward: false,
    zoomPercent: 100,
  });
  render(<BrowserSurface tab={tab} />);
  fireEvent.click(
    await screen.findByRole("button", { name: "Ask about this page" }),
  );
  expect(await screen.findByRole("alert")).toHaveTextContent(
    "Browser page or focused Chat changed",
  );
  expect(
    useWorkbenchStore.getState().chatUiById[chat.id]?.attachedContext ?? [],
  ).toEqual([]);
  useWorkbenchStore.setState({
    project: previous.project,
    tabs: previous.tabs,
    activeTabId: previous.activeTabId,
    chatUiById: previous.chatUiById,
  });
});

it.each([
  ["same-URL reload", "2"],
  ["A to B to A navigation", "3"],
])("rejects page capture after %s", async (_case, afterRevision) => {
  const previous = useWorkbenchStore.getState();
  const chat = { ...tab, id: "chat-reloaded", kind: "chat" as const };
  useWorkbenchStore.setState({
    project: { id: "project-reloaded" } as typeof previous.project,
    tabs: [...previous.tabs, chat],
    activeTabId: tab.id,
  });
  pinAgentChatOwner(chat.id);
  browserPageContext.mockResolvedValue({
    kind: "browserPage",
    tabId: tab.id,
    url: "https://example.com/path",
    title: "Old page",
    text: "Old text",
  });
  render(<BrowserSurface tab={tab} />);
  const ask = await screen.findByRole("button", {
    name: "Ask about this page",
  });
  await waitFor(() => expect(browserStatus).toHaveBeenCalled());
  browserStatus.mockClear();
  browserStatus.mockResolvedValueOnce({
    tabId: tab.id,
    url: "https://example.com/path",
    revision: "1",
    loading: false,
    canGoBack: false,
    canGoForward: false,
    zoomPercent: 100,
  });
  browserStatus.mockResolvedValueOnce({
    tabId: tab.id,
    url: "https://example.com/path",
    revision: afterRevision,
    loading: false,
    canGoBack: false,
    canGoForward: false,
    zoomPercent: 100,
  });
  fireEvent.click(ask);
  expect(await screen.findByRole("alert")).toHaveTextContent(
    "Browser page or focused Chat changed",
  );
  expect(browserPageContext).toHaveBeenCalledWith(tab.id, "1");
  expect(
    useWorkbenchStore.getState().chatUiById[chat.id]?.attachedContext ?? [],
  ).toEqual([]);
  useWorkbenchStore.setState({
    project: previous.project,
    tabs: previous.tabs,
    activeTabId: previous.activeTabId,
    chatUiById: previous.chatUiById,
  });
});

it("rejects captured page if the user switches tabs during capture", async () => {
  const previous = useWorkbenchStore.getState();
  const chat = { ...tab, id: "chat-tab-switch", kind: "chat" as const };
  useWorkbenchStore.setState({
    project: { id: "project-tab-switch" } as typeof previous.project,
    tabs: [...previous.tabs, chat],
    activeTabId: tab.id,
  });
  pinAgentChatOwner(chat.id);
  let resolveCapture!: (value: unknown) => void;
  browserPageContext.mockReturnValue(
    new Promise((resolve) => {
      resolveCapture = resolve;
    }),
  );
  render(<BrowserSurface tab={tab} />);
  fireEvent.click(
    await screen.findByRole("button", { name: "Ask about this page" }),
  );
  await waitFor(() => expect(browserPageContext).toHaveBeenCalled());
  useWorkbenchStore.setState({ activeTabId: chat.id });
  await act(async () =>
    resolveCapture({
      kind: "browserPage",
      tabId: tab.id,
      url: "https://example.com/path",
      title: "Old page",
      text: "Old text",
    }),
  );
  expect(await screen.findByRole("alert")).toHaveTextContent(
    "Browser page or focused Chat changed",
  );
  expect(
    useWorkbenchStore.getState().chatUiById[chat.id]?.attachedContext ?? [],
  ).toEqual([]);
  useWorkbenchStore.setState({
    project: previous.project,
    tabs: previous.tabs,
    activeTabId: previous.activeTabId,
    chatUiById: previous.chatUiById,
  });
});

it("stops the tab's current authority even when presence names a previous chat", async () => {
  const previous = useWorkbenchStore.getState();
  useWorkbenchStore.setState({
    project: { id: "project-1" } as typeof previous.project,
    tabs: [
      ...previous.tabs,
      {
        id: "chat-1",
        kind: "chat",
        title: "Research chat",
        position: 1,
        pinned: false,
        dirty: false,
      },
    ],
  });
  rustAgentControl = {
    projectId: "project-1",
    chatId: "chat-1",
    userControl: false,
    developerMode: false,
  };
  recordBrowserAgentOperation({
    ...browserOperationFixture,
    tabId: tab.id,
    phase: "started",
    updatedAt: Date.now(),
  });
  render(<BrowserSurface tab={tab} />);
  expect(
    await screen.findByText(
      "Agent has a browser action pending · Research chat",
    ),
  ).toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "Stop agent" }));
  fireEvent.click(screen.getByRole("button", { name: "Stop agent" }));
  await waitFor(() => expect(stopBrowserTab).toHaveBeenCalledWith(tab.id));
  expect(stopBrowserTab).toHaveBeenCalledTimes(1);
  expect(detachBrowserTab).not.toHaveBeenCalled();
  await waitFor(() =>
    expect(screen.queryByRole("button", { name: "Stop agent" })).toBeNull(),
  );
  act(() =>
    recordBrowserAgentOperation({
      ...browserOperationFixture,
      tabId: tab.id,
      phase: "outcomeUnknown",
      updatedAt: Date.now() + 1,
    }),
  );
  await waitFor(() =>
    expect(browserAgentControl).toHaveBeenLastCalledWith(
      tab.id,
      "agentControl",
    ),
  );
  expect(screen.queryByText(/Agent attached/)).toBeNull();
  useWorkbenchStore.setState({
    project: previous.project,
    tabs: previous.tabs,
  });
});

it("takes over the tab until the user hands it back", async () => {
  const previous = useWorkbenchStore.getState();
  useWorkbenchStore.setState({
    project: { id: "project-1" } as typeof previous.project,
    tabs: [
      ...previous.tabs,
      {
        id: "chat-1",
        kind: "chat",
        title: "Research chat",
        position: 1,
        pinned: false,
        dirty: false,
      },
    ],
  });
  rustAgentControl = {
    projectId: "project-1",
    chatId: "chat-1",
    userControl: false,
    developerMode: false,
  };
  recordBrowserAgentOperation({
    ...browserOperationFixture,
    operationId: "take-over-operation",
    tabId: tab.id,
    phase: "started",
    updatedAt: Date.now(),
  });
  const view = render(<BrowserSurface tab={tab} />);
  fireEvent.click(await screen.findByRole("button", { name: "Take over" }));
  expect(
    await screen.findByText("You're in control · Research chat"),
  ).toBeInTheDocument();
  expect(browserAgentControl).toHaveBeenCalledWith(tab.id, "agentTakeOver");
  act(() =>
    recordBrowserAgentOperation({
      ...browserOperationFixture,
      operationId: "take-over-operation",
      tabId: tab.id,
      phase: "denied",
      updatedAt: Date.now() + 1,
    }),
  );
  expect(
    await screen.findByText("You're in control · Research chat"),
  ).toBeInTheDocument();
  // A reloaded chrome reads user control back from Rust.
  view.unmount();
  render(<BrowserSurface tab={tab} />);
  expect(
    await screen.findByText("You're in control · Research chat"),
  ).toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "Hand back" }));
  await waitFor(() =>
    expect(browserAgentControl).toHaveBeenCalledWith(tab.id, "agentHandBack"),
  );
  expect(
    await screen.findByRole("button", { name: "Take over" }),
  ).toBeInTheDocument();
  expect(stopBrowserTab).not.toHaveBeenCalled();
  useWorkbenchStore.setState({
    project: previous.project,
    tabs: previous.tabs,
  });
});

it("offers Stop and Take over for as long as an Agent is attached, idle or not", async () => {
  const previous = useWorkbenchStore.getState();
  useWorkbenchStore.setState({
    project: { id: "project-1" } as typeof previous.project,
    tabs: [
      ...previous.tabs,
      {
        id: "chat-1",
        kind: "chat",
        title: "Research chat",
        position: 1,
        pinned: false,
        dirty: false,
      },
    ],
  });
  rustAgentControl = {
    projectId: "project-1",
    chatId: "chat-1",
    userControl: false,
    developerMode: false,
  };
  render(<BrowserSurface tab={tab} />);
  expect(
    await screen.findByText("Agent attached · Research chat"),
  ).toBeInTheDocument();
  expect(screen.getByRole("button", { name: "Take over" })).toBeEnabled();
  expect(screen.getByRole("button", { name: "Stop agent" })).toBeEnabled();
  useWorkbenchStore.setState({
    project: previous.project,
    tabs: previous.tabs,
  });
});

it("keeps page attachment, agent presence, and downloads in one toolbar row", async () => {
  const previous = useWorkbenchStore.getState();
  useWorkbenchStore.setState({
    project: { id: "project-1" } as typeof previous.project,
    tabs: [
      { ...tab, id: "chat-toolbar", kind: "chat", title: "Research chat" },
    ],
  });
  rustAgentControl = {
    projectId: "project-1",
    chatId: "chat-toolbar",
    userControl: false,
    developerMode: false,
  };
  recordBrowserAgentOperation({
    ...browserOperationFixture,
    operationId: "toolbar-operation",
    tabId: tab.id,
    phase: "started",
    updatedAt: Date.now(),
  });
  const view = render(<BrowserSurface tab={tab} />);
  const toolbar = view.container.querySelector(".browser-surface__toolbar");
  expect(
    await screen.findByRole("button", { name: "Attach tab to chat" }),
  ).toBeInTheDocument();
  expect(
    screen
      .getByRole("button", { name: "Attach tab to chat" })
      .closest(".browser-surface__toolbar"),
  ).toBe(toolbar);
  expect(screen.getByRole("status").closest(".browser-surface__toolbar")).toBe(
    toolbar,
  );
  expect(
    screen
      .getByRole("button", { name: "Stop agent" })
      .closest(".browser-surface__toolbar"),
  ).toBe(toolbar);
  expect(
    screen
      .getByRole("button", { name: "Downloads" })
      .closest(".browser-surface__toolbar"),
  ).toBe(toolbar);
  expect(view.container.querySelector(".browser-surface__context")).toBeNull();
  useWorkbenchStore.setState({
    project: previous.project,
    tabs: previous.tabs,
  });
});

it("covers the native page while a toolbar popover is open, even across a resize, without hiding it for real", async () => {
  vi.spyOn(HTMLElement.prototype, "getBoundingClientRect").mockReturnValue(
    DOMRect.fromRect({ x: 0, y: 0, width: 400, height: 300 }),
  );
  render(<BrowserSurface tab={tab} />);
  await waitFor(() => expect(positionBrowser).toHaveBeenCalled());

  hideBrowser.mockClear();
  coverBrowser.mockClear();
  positionBrowser.mockClear();
  fireEvent.click(screen.getByRole("button", { name: "Downloads" }));
  await waitFor(() => expect(coverBrowser).toHaveBeenCalledWith(tab.id));
  act(() => {
    window.dispatchEvent(new Event("resize"));
  });
  await new Promise((resolve) => requestAnimationFrame(resolve));
  expect(positionBrowser).not.toHaveBeenCalled();

  fireEvent.keyDown(screen.getByRole("dialog", { name: "Downloads" }), {
    key: "Escape",
  });
  await waitFor(() => expect(positionBrowser).toHaveBeenCalled());

  coverBrowser.mockClear();
  positionBrowser.mockClear();
  fireEvent.click(screen.getByRole("button", { name: "More options" }));
  await waitFor(() => expect(coverBrowser).toHaveBeenCalledWith(tab.id));
  fireEvent.click(screen.getByRole("button", { name: "More options" }));
  await waitFor(() => expect(positionBrowser).toHaveBeenCalled());
  // A real hide ends Annotate; a popover opening or closing must not.
  expect(hideBrowser).not.toHaveBeenCalled();
});

it("keeps the native page visible under a non-overlapping menu, and covers it only if the menu moves over the page", async () => {
  let overlaps = false;
  let nativePage = "hidden";
  positionBrowser.mockImplementation(async () => {
    nativePage = "visible";
  });
  coverBrowser.mockImplementation(async () => {
    nativePage = "covered";
  });
  vi.spyOn(HTMLElement.prototype, "getBoundingClientRect").mockImplementation(
    function (this: HTMLElement) {
      return DOMRect.fromRect(
        this.matches('[role="menu"]')
          ? { x: overlaps ? 520 : 0, y: 120, width: 200, height: 200 }
          : { x: 500, y: 100, width: 400, height: 300 },
      );
    },
  );
  try {
    render(<BrowserSurface tab={tab} />);
    await waitFor(() => expect(nativePage).toBe("visible"));
    fireEvent.click(screen.getByRole("button", { name: "More options" }));
    await act(async () => {
      await new Promise(requestAnimationFrame);
    });
    expect(screen.getByRole("menu")).toBeVisible();
    expect(nativePage).toBe("visible");
    overlaps = true;
    fireEvent(window, new Event("resize"));
    await waitFor(() => expect(nativePage).toBe("covered"));
    fireEvent.keyDown(screen.getByRole("menu"), { key: "Escape" });
    await waitFor(() => expect(nativePage).toBe("visible"));
  } finally {
    positionBrowser.mockResolvedValue(undefined);
    coverBrowser.mockResolvedValue(undefined);
  }
});

it("keeps the native page covered until the clear-data confirmation is dismissed", async () => {
  let nativePage: "hidden" | "visible" | "covered" = "hidden";
  positionBrowser.mockImplementation(async () => {
    nativePage = "visible";
  });
  coverBrowser.mockImplementation(async () => {
    nativePage = "covered";
  });
  vi.spyOn(HTMLElement.prototype, "getBoundingClientRect").mockReturnValue(
    DOMRect.fromRect({ x: 0, y: 0, width: 400, height: 300 }),
  );
  const paint = () =>
    act(async () => {
      await new Promise<void>((resolve) =>
        requestAnimationFrame(() => resolve()),
      );
    });
  try {
    render(<BrowserSurface tab={tab} />);
    await waitFor(() => expect(nativePage).toBe("visible"));
    fireEvent.click(screen.getByRole("button", { name: "More options" }));
    fireEvent.click(
      screen.getByRole("menuitem", { name: "Clear browsing data" }),
    );
    expect(
      screen.getByRole("dialog", { name: "Clear browsing data?" }),
    ).toBeVisible();
    await paint();
    expect(nativePage).toBe("covered");
    fireEvent(window, new Event("resize"));
    await paint();
    expect(nativePage).toBe("covered");
    fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
    await waitFor(() => expect(nativePage).toBe("visible"));
  } finally {
    positionBrowser.mockResolvedValue(undefined);
    coverBrowser.mockResolvedValue(undefined);
  }
});

it("still hides the native page for real on unmount or a tab switch while a popover covers it", async () => {
  vi.spyOn(HTMLElement.prototype, "getBoundingClientRect").mockReturnValue(
    DOMRect.fromRect({ x: 0, y: 0, width: 400, height: 300 }),
  );
  const view = render(<BrowserSurface tab={tab} />);
  await waitFor(() => expect(positionBrowser).toHaveBeenCalled());
  hideBrowser.mockClear();
  coverBrowser.mockClear();
  fireEvent.click(screen.getByRole("button", { name: "More options" }));
  await waitFor(() => expect(coverBrowser).toHaveBeenCalledWith(tab.id));
  expect(hideBrowser).not.toHaveBeenCalled();
  view.rerender(<BrowserSurface tab={{ ...tab, id: "browser-2" }} />);
  expect(hideBrowser).toHaveBeenCalledWith(tab.id);

  // The menu stays open in the new tab, which is covered from the start.
  expect(screen.getByRole("menu")).toBeInTheDocument();
  await waitFor(() => expect(coverBrowser).toHaveBeenCalledWith("browser-2"));
  hideBrowser.mockClear();
  view.unmount();
  expect(hideBrowser).toHaveBeenCalledWith("browser-2");
});

it("still hides the native page for real when the centre pane is hidden", async () => {
  vi.spyOn(HTMLElement.prototype, "getBoundingClientRect").mockReturnValue(
    DOMRect.fromRect({ x: 0, y: 0, width: 400, height: 300 }),
  );
  const center = document.createElement("div");
  center.className = "center-region";
  document.body.append(center);
  render(<BrowserSurface tab={tab} />, { container: center });
  await waitFor(() => expect(positionBrowser).toHaveBeenCalled());

  hideBrowser.mockClear();
  coverBrowser.mockClear();
  fireEvent.click(screen.getByRole("button", { name: "More options" }));
  await waitFor(() => expect(coverBrowser).toHaveBeenCalledWith(tab.id));
  act(() => center.setAttribute("aria-hidden", "true"));
  await waitFor(() => expect(hideBrowser).toHaveBeenCalledWith(tab.id));
  center.remove();
});

it("hides the native page under History and opens a chosen visit in this tab", async () => {
  vi.spyOn(HTMLElement.prototype, "getBoundingClientRect").mockReturnValue(
    DOMRect.fromRect({ x: 0, y: 0, width: 400, height: 300 }),
  );
  render(<BrowserSurface tab={tab} />);
  await waitFor(() => expect(positionBrowser).toHaveBeenCalled());
  hideBrowser.mockClear();
  fireEvent.click(screen.getByRole("button", { name: "More options" }));
  fireEvent.click(screen.getByRole("menuitem", { name: "History" }));
  const history = await screen.findByRole("region", { name: "History" });
  positionBrowser.mockClear();
  // History stays up indefinitely, so the page hides for real (ending
  // Annotate, as a tab switch does) rather than being briefly covered.
  await waitFor(() => expect(hideBrowser).toHaveBeenCalledWith(tab.id));
  expect(positionBrowser).not.toHaveBeenCalled();
  expect(browserHistoryPage).toHaveBeenCalledWith({
    search: "",
    before: null,
    limit: 50,
  });
  fireEvent.click(
    await within(history).findByRole("button", {
      name: /^(?!Remove ).*History Example/,
    }),
  );
  await waitFor(() =>
    expect(browserNavigate).toHaveBeenCalledWith(
      tab.id,
      "https://history.example/page",
    ),
  );
  expect(screen.queryByRole("region", { name: "History" })).toBeNull();
  await waitFor(() => expect(positionBrowser).toHaveBeenCalled());
});

it("opens History with Ctrl+H and closes it with Escape even while the page loads", async () => {
  browserStatus.mockResolvedValue({
    tabId: tab.id,
    url: "https://example.com/path",
    revision: "1",
    loading: true,
    canGoBack: false,
    canGoForward: false,
    zoomPercent: 100,
  });
  render(<BrowserSurface tab={tab} />);
  const address = await screen.findByRole("textbox", {
    name: "Address or search",
  });
  fireEvent.keyDown(address, { key: "h", ctrlKey: true });
  const search = await screen.findByRole("searchbox", {
    name: "Search history",
  });
  await waitFor(() => expect(search).toHaveFocus());
  fireEvent.keyDown(search, { key: "Escape" });
  expect(screen.queryByRole("region", { name: "History" })).toBeNull();
  expect(browserControl).not.toHaveBeenCalledWith(tab.id, "stop");
  await waitFor(() => expect(address).toHaveFocus());
});

it("shows a failed attach in the surface's error row and clears it on a successful retry", async () => {
  const previous = useWorkbenchStore.getState();
  useWorkbenchStore.setState({
    project: { id: "project-1" } as typeof previous.project,
    tabs: [{ ...tab, id: "chat-attach", kind: "chat", title: "Research chat" }],
  });
  attachBrowserTab.mockRejectedValueOnce(new Error("native"));
  const view = render(<BrowserSurface tab={tab} />);
  const attach = await screen.findByRole("button", {
    name: "Attach tab to chat",
  });
  await waitFor(() => expect(attach).toBeEnabled());
  fireEvent.click(attach);
  const error = await screen.findByRole("alert");
  expect(error).toHaveTextContent(
    "Could not attach this tab to the selected chat.",
  );
  // The row sits in the surface's layout between the toolbar and the page,
  // not inside the toolbar, so it pushes the native page down.
  expect(error.closest(".browser-surface__toolbar")).toBeNull();
  const toolbar = view.container.querySelector(".browser-surface__toolbar")!;
  expect(
    toolbar.compareDocumentPosition(error) & Node.DOCUMENT_POSITION_FOLLOWING,
  ).toBeTruthy();

  fireEvent.click(attach);
  await screen.findByRole("button", { name: "Detach tab from chat" });
  expect(screen.queryByRole("alert")).toBeNull();
  useWorkbenchStore.setState({
    project: previous.project,
    tabs: previous.tabs,
  });
});
