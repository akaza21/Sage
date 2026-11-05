import { useAgentSettingsState } from "../settings/agentSettingsState";
import { readFile } from "node:fs/promises";
import { resolve } from "node:path";
import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import type { WorkbenchTab } from "../../domain/workbench";
import { BrowserSurface } from "./BrowserSurface";
import { BrowserNewTab } from "./BrowserNewTab";
import { useWorkbenchStore } from "../../app/store";
import { pinAgentChatOwner } from "../../app/agentChatOwner";
import { useBrowserPreferencesStore } from "./browserPreferencesStore";
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
const browserPrint = vi.fn(async (_tabId: string) => undefined);
const browserCaptureScreenshot = vi.fn(
  async (_tabId: string, _revision: string) => ({
    width: 1,
    height: 1,
    pngBase64: "iVBORw0KGgo=",
  }),
);
const browserNavigate = vi.fn(
  async (_tabId: string, _url: string) => undefined,
);
const hideBrowser = vi.fn(async (_tabId: string) => undefined);
const positionBrowser = vi.fn(
  async (_tabId: string, _bounds: unknown) => undefined,
);
const browserSuggestedSites = vi.fn(async () => ["https://example.com/"]);
const browserHistory = vi.fn(async (_tabId: string) => [
  "https://example.com/",
]);
const detachBrowserTab = vi.fn(
  async (_projectId: string, _chatId: string) => undefined,
);
const stopBrowserTab = vi.fn(async (_tabId: string) => undefined);
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
  browserPrint: (tabId: string) => browserPrint(tabId),
  browserCaptureScreenshot: (tabId: string, revision: string) =>
    browserCaptureScreenshot(tabId, revision),
  browserSuggestedSites: () => browserSuggestedSites(),
  browserHistory: (tabId: string) => browserHistory(tabId),
  attachBrowserTab: vi.fn(),
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
  browserControl: vi.fn(),
  browserAgentControl: vi.fn(async () => null),
  browserNavigate: (tabId: string, url: string) => browserNavigate(tabId, url),
  hideBrowser: (tabId: string) => hideBrowser(tabId),
  coverBrowser: vi.fn(async () => undefined),
  positionBrowser: (tabId: string, bounds: unknown) =>
    positionBrowser(tabId, bounds),
}));
const browserPreferences = vi.fn(async () => ({ showFullUrl: false }));
vi.mock("../../ports/nativeBrowserPreferencesPort", () => ({
  browserPreferences: () => browserPreferences(),
  browserSetPreferences: vi.fn(async (value: unknown) => value),
}));
vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(async () => () => undefined),
}));
const tab: WorkbenchTab = {
  id: "browser-1",
  kind: "browser",
  title: "Example",
  position: 0,
  pinned: false,
  dirty: false,
};
it("shows an inert new tab with Sage tools and suggested sites", async () => {
  browserStatus.mockResolvedValue({
    tabId: tab.id,
    url: "",
    revision: "0",
    loading: false,
    canGoBack: false,
    canGoForward: false,
    zoomPercent: 100,
  });
  render(<BrowserSurface tab={tab} />);
  expect(await screen.findByText("Tools")).toBeInTheDocument();
  expect(screen.getByText("Suggested")).toBeInTheDocument();
  expect(
    screen.getByPlaceholderText("Search or enter a URL"),
  ).toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "More options" }));
  expect(
    screen.getByRole("menuitem", { name: "Take a screenshot" }),
  ).toBeDisabled();
  expect(screen.getByRole("menuitem", { name: "Find in page" })).toBeDisabled();
  expect(screen.getByRole("menuitem", { name: "Print" })).toBeDisabled();
  expect(screen.getByRole("menuitem", { name: "Zoom out" })).toBeDisabled();
  expect(
    screen.getByRole("menuitem", { name: "Show device toolbar" }),
  ).toBeDisabled();
  expect(screen.getByRole("menuitem", { name: "Downloads" })).toBeEnabled();
  await waitFor(() =>
    expect(screen.getByRole("menuitem", { name: "Downloads" })).toHaveFocus(),
  );
  fireEvent.click(screen.getByRole("button", { name: "More options" }));
  fireEvent.click(await screen.findByRole("button", { name: "example.com" }));
  await waitFor(() =>
    expect(browserNavigate).toHaveBeenCalledWith(
      tab.id,
      "https://example.com/",
    ),
  );
});
it("shows a known open page title on its suggested tile", async () => {
  const previous = useWorkbenchStore.getState();
  useWorkbenchStore.setState({ tabs: [tab] });
  browserStatus.mockImplementation(async (tabId: string) => ({
    tabId,
    url: tabId === tab.id ? "https://example.com/" : "",
    title: tabId === tab.id ? "Example Domain" : null,
    revision: "1",
    loading: false,
    canGoBack: false,
    canGoForward: false,
    zoomPercent: 100,
  }));
  render(<BrowserSurface tab={{ ...tab, id: "browser-new-tab" }} />);
  expect(
    await screen.findByRole("button", { name: "Example Domain" }),
  ).toBeInTheDocument();
  useWorkbenchStore.setState({ tabs: previous.tabs });
});
it("shows a known favicon on its suggested tile and keeps the glyph fallback", async () => {
  const previous = useWorkbenchStore.getState();
  useWorkbenchStore.setState({ tabs: [tab] });
  browserSuggestedSites.mockResolvedValueOnce([
    "https://example.com/",
    "https://plain.example/",
  ]);
  browserStatus.mockImplementation(async (tabId: string) => ({
    tabId,
    url: tabId === tab.id ? "https://example.com/" : "",
    title: "Example Domain",
    favicon: "data:image/png;base64,iVBORw0KGgo=",
    revision: "1",
    loading: false,
    canGoBack: false,
    canGoForward: false,
    zoomPercent: 100,
  }));
  render(<BrowserSurface tab={{ ...tab, id: "browser-new-tab" }} />);
  await waitFor(() => expect(browserSuggestedSites).toHaveBeenCalledTimes(1));
  expect(await browserSuggestedSites.mock.results[0]?.value).toEqual([
    "https://example.com/",
    "https://plain.example/",
  ]);
  const illustrated = await screen.findByRole("button", {
    name: "Example Domain",
  });
  expect(illustrated.querySelector("img")).toHaveAttribute(
    "src",
    "data:image/png;base64,iVBORw0KGgo=",
  );
  expect(
    screen.getByRole("button", { name: "plain.example" }).querySelector("img"),
  ).toBeNull();
  useWorkbenchStore.setState({ tabs: previous.tabs });
});
it("falls back when a suggested favicon cannot decode and retries a changed URL", () => {
  const first = "data:image/png;base64,iVBORw0KGgo=";
  const next = "data:image/png;base64,iVBORw0KGgoAAA=";
  const props = {
    onNavigate: vi.fn(),
    onNewTab: vi.fn(),
    onTool: vi.fn(),
    chatAvailable: true,
    sideChatAvailable: false,
    terminalAvailable: false,
  };
  const view = render(
    <BrowserNewTab
      {...props}
      suggestions={[{ url: "https://example.com/", favicon: first }]}
    />,
  );
  const tile = screen.getByRole("button", { name: "example.com" });
  fireEvent.error(tile.querySelector("img")!);
  expect(tile.querySelector("img")).toBeNull();
  expect(tile).toHaveTextContent("◎");
  view.rerender(
    <BrowserNewTab
      {...props}
      suggestions={[{ url: "https://example.com/", favicon: next }]}
    />,
  );
  expect(tile.querySelector("img")).toHaveAttribute("src", next);
});
it("replaces suggested sites when switching projects between new tabs", async () => {
  const previous = useWorkbenchStore.getState();
  browserSuggestedSites.mockImplementation(async () =>
    useWorkbenchStore.getState().project?.id === "project-b"
      ? ["https://project-b.example/"]
      : ["https://project-a.example/"],
  );
  browserStatus.mockImplementation(async (tabId: string) => ({
    tabId,
    url: "",
    revision: "0",
    loading: false,
    canGoBack: false,
    canGoForward: false,
    zoomPercent: 100,
  }));
  useWorkbenchStore.setState({
    project: { id: "project-a" } as typeof previous.project,
  });
  const view = render(<BrowserSurface tab={{ ...tab, id: "browser-a" }} />);
  expect(
    await screen.findByRole("button", { name: "project-a.example" }),
  ).toBeInTheDocument();
  useWorkbenchStore.setState({
    project: { id: "project-b" } as typeof previous.project,
  });
  view.rerender(<BrowserSurface tab={{ ...tab, id: "browser-b" }} />);
  expect(
    await screen.findByRole("button", { name: "project-b.example" }),
  ).toBeInTheDocument();
  expect(
    screen.queryByRole("button", { name: "project-a.example" }),
  ).toBeNull();
  useWorkbenchStore.setState({ project: previous.project });
});
it("opens the UI New tab action without a URL and renders native blank state", async () => {
  const previous = useWorkbenchStore.getState();
  useWorkbenchStore.setState({
    project: { id: "project-new-tab", name: "Project", roots: [] },
    projectTransitioning: false,
    tabs: [],
    activeTabId: null,
  });
  browserStatus.mockResolvedValueOnce({
    tabId: "browser-new",
    url: "",
    revision: "0",
    loading: false,
    canGoBack: false,
    canGoForward: false,
    zoomPercent: 100,
  });
  await act(async () => useWorkbenchStore.getState().openBrowser());
  expect(openBrowser).toHaveBeenCalledWith(undefined);
  const newTab = useWorkbenchStore
    .getState()
    .tabs.find((item) => item.id === "browser-new");
  expect(newTab?.title).toBe("New tab");
  render(<BrowserSurface tab={newTab!} />);
  expect(await screen.findByText("Tools")).toBeInTheDocument();
  useWorkbenchStore.setState({
    project: previous.project,
    tabs: previous.tabs,
    activeTabId: previous.activeTabId,
  });
});
it("labels a committed browser tab with its host instead of a generic Browser title", async () => {
  const previous = useWorkbenchStore.getState();
  useWorkbenchStore.setState({ tabs: [tab] });
  render(<BrowserSurface tab={tab} />);
  await waitFor(() =>
    expect(useWorkbenchStore.getState().tabs[0]?.title).toBe("example.com"),
  );
  useWorkbenchStore.setState({ tabs: previous.tabs });
});
it("uses the bounded document title and falls back to the host", () => {
  const previous = useWorkbenchStore.getState();
  useWorkbenchStore.setState({ tabs: [tab] });
  const update = useWorkbenchStore.getState().updateBrowserTabTitle;
  update(
    tab.id,
    "https://example.com/article",
    "Example Domain\u0000\n Guide",
    "1",
  );
  expect(useWorkbenchStore.getState().tabs[0]?.title).toBe(
    "Example Domain Guide",
  );
  update(tab.id, "https://example.com/article", "\u0000\n", "2");
  expect(useWorkbenchStore.getState().tabs[0]?.title).toBe("example.com");
  update(tab.id, "https://example.com/article", "x".repeat(200), "3");
  expect(useWorkbenchStore.getState().tabs[0]?.title).toHaveLength(80);
  useWorkbenchStore.setState({ tabs: previous.tabs });
});
it("applies a native browser status title to the workbench tab", async () => {
  const previous = useWorkbenchStore.getState();
  useWorkbenchStore.setState({ tabs: [tab] });
  browserStatus.mockResolvedValueOnce({
    tabId: tab.id,
    url: "https://example.com/",
    title: "Example Domain",
    revision: "2",
    loading: false,
    canGoBack: false,
    canGoForward: false,
    zoomPercent: 100,
  });
  render(<BrowserSurface tab={tab} />);
  await waitFor(() =>
    expect(useWorkbenchStore.getState().tabs[0]?.title).toBe("Example Domain"),
  );
  useWorkbenchStore.setState({ tabs: previous.tabs });
});
it("selects the full committed URL when the centered host is focused", async () => {
  render(<BrowserSurface tab={tab} />);
  const address = await screen.findByRole<HTMLInputElement>("textbox", {
    name: "Address or search",
  });
  await waitFor(() => expect(address).toHaveValue("example.com"));

  fireEvent.focus(address);

  expect(address).toHaveValue("https://example.com/path");
  expect(address.selectionStart).toBe(0);
  expect(address.selectionEnd).toBe("https://example.com/path".length);
});
it("shows the full committed URL when Show full URL is on", async () => {
  useBrowserPreferencesStore.setState({
    preferences: { showFullUrl: true },
    ready: true,
    error: null,
  });
  try {
    render(<BrowserSurface tab={tab} />);
    const address = await screen.findByRole<HTMLInputElement>("textbox", {
      name: "Address or search",
    });
    await waitFor(() =>
      expect(address).toHaveValue("https://example.com/path"),
    );
    act(() =>
      useBrowserPreferencesStore.setState({
        preferences: { showFullUrl: false },
      }),
    );
    expect(address).toHaveValue("example.com");
  } finally {
    useBrowserPreferencesStore.setState({
      preferences: { showFullUrl: false },
      ready: false,
      error: null,
    });
  }
});
it("loads the stored Show full URL choice when the browser opens", async () => {
  render(<BrowserSurface tab={tab} />);
  await screen.findByRole("textbox", { name: "Address or search" });
  expect(browserPreferences).toHaveBeenCalled();
});
it("opens the shared Browser settings dialog from the menu", async () => {
  render(<BrowserSurface tab={tab} />);
  await screen.findByRole("textbox", { name: "Address or search" });
  fireEvent.click(screen.getByRole("button", { name: "More options" }));
  fireEvent.click(screen.getByRole("menuitem", { name: "Browser settings" }));
  expect(useAgentSettingsState.getState()).toMatchObject({
    page: "Browser",
    browserTabId: tab.id,
    showSitePermissions: false,
  });
});
it("keeps this tab's site permissions one menu item away in the shared dialog", async () => {
  render(<BrowserSurface tab={tab} />);
  await screen.findByRole("textbox", { name: "Address or search" });
  fireEvent.click(screen.getByRole("button", { name: "More options" }));
  fireEvent.click(screen.getByRole("menuitem", { name: "Site settings" }));
  expect(useAgentSettingsState.getState()).toMatchObject({
    page: "Browser",
    browserTabId: tab.id,
    showSitePermissions: true,
  });
});
it("groups supported page controls in an overflow menu without dead features", async () => {
  render(<BrowserSurface tab={tab} />);
  await screen.findByRole("textbox", { name: "Address or search" });
  fireEvent.click(screen.getByRole("button", { name: "More options" }));
  expect(
    screen.getByRole("menuitem", { name: "Find in page" }),
  ).toBeInTheDocument();
  expect(screen.getByRole("menuitem", { name: "History" })).toBeInTheDocument();
  expect(
    screen.getByRole("menuitem", { name: "Downloads" }),
  ).toBeInTheDocument();
  expect(
    screen.getByRole("menuitem", { name: "Browser settings" }),
  ).toBeInTheDocument();
  expect(screen.getByRole("menuitem", { name: "Print" })).toBeInTheDocument();
  expect(
    screen.getByRole("menuitem", { name: "Show device toolbar" }),
  ).toBeEnabled();
  expect(
    screen.getByRole("menuitem", { name: "Take a screenshot" }),
  ).toBeInTheDocument();
  expect(screen.queryByText("Import cookies and passwords...")).toBeNull();
  fireEvent.click(screen.getByRole("menuitem", { name: "Find in page" }));
  expect(
    await screen.findByRole("textbox", { name: "Find text" }),
  ).toHaveFocus();
});
it("opens native print for the loaded tab from the menu", async () => {
  browserPrint.mockClear();
  render(<BrowserSurface tab={tab} />);
  await screen.findByLabelText("Secure connection");
  fireEvent.click(screen.getByRole("button", { name: "More options" }));
  fireEvent.click(screen.getByRole("menuitem", { name: "Print" }));
  await waitFor(() => expect(browserPrint).toHaveBeenCalledWith(tab.id));
  expect(screen.queryByRole("menu", { name: "Browser menu" })).toBeNull();
});
it("renders the secure address indicator as a lean lock icon", async () => {
  render(<BrowserSurface tab={tab} />);
  const indicator = await screen.findByLabelText("Secure connection");
  expect(indicator.querySelector("svg.lucide-lock")).not.toBeNull();
  expect(indicator.textContent).toBe("");
});
it("copies an explicit loaded-page screenshot and confirms the result", async () => {
  const write = vi.fn(async (_items: ClipboardItem[]) => undefined);
  vi.stubGlobal("navigator", { ...navigator, clipboard: { write } });
  vi.stubGlobal(
    "ClipboardItem",
    class {
      constructor(public readonly items: Record<string, Blob>) {}
      async getType(type: string) {
        return this.items[type];
      }
    },
  );
  render(<BrowserSurface tab={tab} />);
  await screen.findByLabelText("Secure connection");

  fireEvent.click(screen.getByRole("button", { name: "More options" }));
  fireEvent.click(screen.getByRole("menuitem", { name: "Take a screenshot" }));

  await waitFor(() =>
    expect(browserCaptureScreenshot).toHaveBeenCalledWith(tab.id, "1"),
  );
  await waitFor(() => expect(write).toHaveBeenCalledTimes(1));
  expect(await write.mock.calls[0]?.[0][0].getType("image/png")).toHaveProperty(
    "type",
    "image/png",
  );
  expect(await screen.findByRole("status")).toHaveTextContent(
    "Screenshot copied to clipboard",
  );
});
it("shows a browser error when the clipboard denies a screenshot write", async () => {
  const write = vi.fn(async () => {
    throw new Error("denied");
  });
  vi.stubGlobal("navigator", { ...navigator, clipboard: { write } });
  vi.stubGlobal(
    "ClipboardItem",
    class {
      constructor(public readonly items: Record<string, Blob>) {}
    },
  );
  render(<BrowserSurface tab={tab} />);
  await screen.findByLabelText("Secure connection");
  fireEvent.click(screen.getByRole("button", { name: "More options" }));
  fireEvent.click(screen.getByRole("menuitem", { name: "Take a screenshot" }));
  expect(await screen.findByRole("alert")).toHaveTextContent(
    "Screenshot could not be copied",
  );
  expect(screen.queryByRole("status")).toBeNull();
});
it("does not confirm a stale screenshot after a pending clipboard write", async () => {
  let finishWrite!: () => void;
  const write = vi.fn(
    () =>
      new Promise<void>((resolve) => {
        finishWrite = resolve;
      }),
  );
  vi.stubGlobal("navigator", { ...navigator, clipboard: { write } });
  vi.stubGlobal(
    "ClipboardItem",
    class {
      constructor(public readonly items: Record<string, Blob>) {}
    },
  );
  render(<BrowserSurface tab={tab} />);
  await screen.findByLabelText("Secure connection");
  fireEvent.click(screen.getByRole("button", { name: "More options" }));
  fireEvent.click(screen.getByRole("menuitem", { name: "Take a screenshot" }));
  await waitFor(() => expect(write).toHaveBeenCalledTimes(1));

  const address = screen.getByRole("textbox", { name: "Address or search" });
  fireEvent.change(address, { target: { value: "https://other.example/" } });
  fireEvent.submit(address.closest("form")!);
  expect(screen.queryByRole("status")).toBeNull();
  await act(async () => finishWrite());
  expect(screen.queryByRole("status")).toBeNull();
});
it("does not alter the clipboard when the page changes before capture", async () => {
  browserCaptureScreenshot.mockRejectedValueOnce(
    new Error("browser page changed; retry screenshot"),
  );
  const write = vi.fn(async () => undefined);
  vi.stubGlobal("navigator", { ...navigator, clipboard: { write } });
  render(<BrowserSurface tab={tab} />);
  await screen.findByLabelText("Secure connection");
  fireEvent.click(screen.getByRole("button", { name: "More options" }));
  fireEvent.click(screen.getByRole("menuitem", { name: "Take a screenshot" }));
  expect(await screen.findByRole("alert")).toHaveTextContent(
    "browser page changed; retry screenshot",
  );
  expect(write).not.toHaveBeenCalled();
  expect(screen.queryByRole("status")).toBeNull();
});
it("explains when system printing is unavailable on this platform", async () => {
  browserPrint.mockClear();
  browserStatus.mockResolvedValueOnce({
    tabId: tab.id,
    url: "https://example.com/path",
    revision: "1",
    loading: false,
    canGoBack: false,
    canGoForward: false,
    zoomPercent: 100,
    printUnavailableReason:
      "system print dialog is unavailable on this platform",
  });
  render(<BrowserSurface tab={tab} />);
  await screen.findByLabelText("Secure connection");
  fireEvent.click(screen.getByRole("button", { name: "More options" }));
  const print = screen.getByRole("menuitem", { name: "Print" });
  expect(print).toBeDisabled();
  expect(print).toHaveAttribute(
    "title",
    "system print dialog is unavailable on this platform",
  );
  fireEvent.click(print);
  expect(browserPrint).not.toHaveBeenCalled();
});
it("dismisses the browser menu with Escape and restores trigger focus", async () => {
  render(<BrowserSurface tab={tab} />);
  const trigger = screen.getByRole("button", { name: "More options" });
  fireEvent.click(trigger);
  const menu = screen.getByRole("menu", { name: "Browser menu" });
  expect(menu).toBeInTheDocument();
  fireEvent.keyDown(menu, { key: "Escape" });
  expect(menu).not.toBeVisible();
  expect(screen.queryByRole("menu", { name: "Browser menu" })).toBeNull();
  expect(trigger).toHaveFocus();
});
it("moves keyboard focus through browser menu items and closes from the trigger", async () => {
  render(<BrowserSurface tab={tab} />);
  await screen.findByLabelText("Secure connection");
  const trigger = screen.getByRole("button", { name: "More options" });
  fireEvent.click(trigger);
  const items = screen.getAllByRole("menuitem");
  expect(items[0]).toHaveFocus();
  expect(
    screen.getByRole("menuitem", { name: "Zoom out" }),
  ).toBeInTheDocument();
  fireEvent.keyDown(items[0], { key: "ArrowDown" });
  expect(items[1]).toHaveFocus();
  fireEvent.keyDown(items[1], { key: "End" });
  expect(items.at(-1)).toHaveFocus();
  fireEvent.keyDown(items.at(-1)!, { key: "Home" });
  expect(items[0]).toHaveFocus();
  trigger.focus();
  fireEvent.keyDown(trigger, { key: "Escape" });
  expect(screen.queryByRole("menu")).toBeNull();
  expect(trigger).toHaveFocus();
});
it("returns focus to the overflow trigger when clearing data is cancelled", async () => {
  render(<BrowserSurface tab={tab} />);
  const trigger = screen.getByRole("button", { name: "More options" });
  fireEvent.click(trigger);
  fireEvent.click(
    screen.getByRole("menuitem", { name: "Clear browsing data" }),
  );
  expect(screen.queryByRole("menu")).toBeNull();
  const dialog = screen.getByRole("dialog", { name: "Clear browsing data?" });
  expect(screen.getByRole("button", { name: "Cancel" })).toHaveFocus();
  fireEvent.keyDown(dialog, { key: "Escape" });
  expect(screen.queryByRole("dialog")).toBeNull();
  expect(trigger).toHaveFocus();
});
it("hides the native browser when its host pane unmounts", () => {
  const view = render(<BrowserSurface tab={tab} />);
  view.unmount();
  expect(hideBrowser).toHaveBeenCalledWith(tab.id);
});
it("contains browser chrome inside the Agent canvas and styles page actions", async () => {
  const css = await readFile(
    resolve("src/features/browser/browser.css"),
    "utf8",
  );
  expect(css).toMatch(
    /\.agent-artifact-pane__canvas > \.browser-surface\s*\{[^}]*position:\s*relative;[^}]*width:\s*100%;[^}]*height:\s*100%;/s,
  );
  expect(css).toMatch(
    /\.browser-surface__attachment\s*\{[^}]*display:\s*flex;[^}]*align-items:\s*center;/s,
  );
  expect(css).toMatch(
    /\.browser-surface__attachment (?:button|select)[^}]*background:\s*var\(--browser-control-fill\);/s,
  );
  expect(css).toMatch(
    /\.browser-surface__menu \[role="menuitem"\]\s*\{[^}]*border:\s*0;[^}]*background:\s*transparent;/s,
  );
  expect(css).toMatch(/\.browser-surface__toolbar\s*\{[^}]*height:\s*44px;/s);
  expect(css).toMatch(
    /\.browser-surface__nav-group\s*\{[^}]*background:\s*var\(--browser-control-fill\);/s,
  );
  expect(css).toMatch(/\.browser-surface__toolbar button:focus-visible\s*\{/s);
  expect(css).toMatch(
    /\.browser-surface__address input\s*\{[^}]*height:\s*32px;[^}]*background:\s*var\(--browser-address-fill\);/s,
  );
  const tokens = await readFile(resolve("src/styles/global.css"), "utf8");
  expect(tokens).toContain("--browser-address-fill: #2d2d2d;");
});

it("clips native webview bounds to the Agent canvas below browser chrome", async () => {
  const rect = (left: number, top: number, width: number, height: number) =>
    ({
      left,
      top,
      width,
      height,
      right: left + width,
      bottom: top + height,
    }) as DOMRect;
  vi.spyOn(HTMLElement.prototype, "getBoundingClientRect").mockImplementation(
    function (this: HTMLElement) {
      if (this.classList.contains("agent-artifact-pane__canvas"))
        return rect(630, 100, 740, 760);
      if (this.classList.contains("browser-surface__toolbar"))
        return rect(630, 100, 740, 40);
      if (this.classList.contains("browser-surface__viewport"))
        return rect(600, 110, 800, 800);
      return rect(0, 0, 0, 0);
    },
  );
  render(
    <div className="agent-artifact-pane__canvas">
      <BrowserSurface tab={tab} />
    </div>,
  );
  await waitFor(() =>
    expect(positionBrowser).toHaveBeenCalledWith(tab.id, {
      x: 630,
      y: 140,
      width: 740,
      height: 720,
    }),
  );
});

it("keeps Editor browser bounds at the frame rect", async () => {
  const rect = {
    left: 42,
    top: 90,
    right: 542,
    bottom: 490,
    width: 500,
    height: 400,
  } as DOMRect;
  vi.spyOn(HTMLElement.prototype, "getBoundingClientRect").mockReturnValue(
    rect,
  );
  render(
    <div className="center-region">
      <BrowserSurface tab={tab} />
    </div>,
  );
  await waitFor(() =>
    expect(positionBrowser).toHaveBeenCalledWith(tab.id, {
      x: 42,
      y: 90,
      width: 500,
      height: 400,
    }),
  );
});

beforeEach(() => {
  vi.clearAllMocks();
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
  pinAgentChatOwner(null);
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
});
it("offers committed origin, copy, external open, zoom and find on the active tab", async () => {
  const clipboard = { writeText: vi.fn(async () => undefined) };
  vi.stubGlobal("navigator", { ...navigator, clipboard });
  render(<BrowserSurface tab={tab} />);

  await screen.findByLabelText("Secure connection");
  fireEvent.click(screen.getByRole("button", { name: "More options" }));
  fireEvent.click(screen.getByRole("menuitem", { name: "Copy URL" }));
  await waitFor(() =>
    expect(clipboard.writeText).toHaveBeenCalledWith(
      "https://example.com/path",
    ),
  );
  fireEvent.click(screen.getByRole("button", { name: "More options" }));
  fireEvent.click(screen.getByRole("menuitem", { name: "Open externally" }));
  await waitFor(() => expect(browserOpenExternal).toHaveBeenCalledWith(tab.id));
  fireEvent.click(screen.getByRole("button", { name: "More options" }));
  fireEvent.click(screen.getByRole("menuitem", { name: "Zoom in" }));
  await waitFor(() => expect(browserZoom).toHaveBeenCalledWith(tab.id, 125));
  fireEvent.click(screen.getByRole("menuitem", { name: "Find in page" }));
  fireEvent.change(await screen.findByRole("textbox", { name: "Find text" }), {
    target: { value: "example" },
  });
  fireEvent.click(screen.getByRole("button", { name: /^Find$/ }));
  await waitFor(() =>
    expect(browserFind).toHaveBeenCalledWith(tab.id, "start", "example"),
  );
});
it("does not share a provisional URL while the page is loading", async () => {
  browserStatus.mockResolvedValueOnce({
    tabId: tab.id,
    url: "https://redirect.example/",
    revision: "2",
    loading: true,
    canGoBack: false,
    canGoForward: false,
    zoomPercent: 100,
  });
  render(<BrowserSurface tab={tab} />);

  await waitFor(() =>
    expect(
      screen.getByRole("textbox", { name: "Address or search" }),
    ).toHaveValue("https://redirect.example/"),
  );
  expect(screen.getByLabelText("No page")).toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "More options" }));
  expect(screen.queryByRole("menuitem", { name: "Copy URL" })).toBeNull();
  expect(
    screen.queryByRole("menuitem", { name: "Open externally" }),
  ).toBeNull();
  expect(
    screen.getByRole("menuitem", { name: "Take a screenshot" }),
  ).toBeDisabled();
});
it("shows a failed history retry without offering a mismatched origin", async () => {
  browserStatus.mockResolvedValueOnce({
    tabId: tab.id,
    url: "",
    revision: "2",
    loading: false,
    canGoBack: true,
    canGoForward: false,
    zoomPercent: 100,
    error: "Back or Forward could not reach the saved page",
  });
  render(<BrowserSurface tab={tab} />);
  expect(await screen.findByRole("alert")).toHaveTextContent(
    "Back or Forward could not reach the saved page",
  );
  expect(screen.getByLabelText("No page")).toBeInTheDocument();
  expect(screen.queryByText("Suggested")).toBeNull();
  fireEvent.click(screen.getByRole("button", { name: "More options" }));
  expect(screen.queryByRole("menuitem", { name: "Copy URL" })).toBeNull();
  expect(
    screen.queryByRole("menuitem", { name: "Open externally" }),
  ).toBeNull();
});

it("discards a closed history drawer's pending page and reopens with fresh native history", async () => {
  let finish!: (entries: string[]) => void;
  browserHistory
    .mockImplementationOnce(
      () =>
        new Promise((resolve) => {
          finish = resolve;
        }),
    )
    .mockResolvedValueOnce(["https://fresh.example/"]);
  render(<BrowserSurface tab={tab} />);
  await screen.findByLabelText("Secure connection");
  const openHistory = () => {
    fireEvent.click(screen.getByRole("button", { name: "More options" }));
    fireEvent.click(screen.getByRole("menuitem", { name: "History" }));
  };
  openHistory();
  await screen.findByRole("complementary", { name: "Browser history" });
  fireEvent.click(screen.getByRole("button", { name: "Close history" }));
  openHistory();
  await screen.findByRole("button", { name: "https://fresh.example/" });
  await act(async () => finish(["https://stale.example/"]));
  expect(screen.queryByText("https://stale.example/")).toBeNull();
  expect(
    screen.getByRole("button", { name: "https://fresh.example/" }),
  ).toBeVisible();
  fireEvent.click(screen.getByRole("button", { name: "Close history" }));
  expect(
    screen.queryByRole("complementary", { name: "Browser history" }),
  ).toBeNull();
});
