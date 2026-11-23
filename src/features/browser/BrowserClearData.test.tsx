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

import { useWorkbenchStore } from "../../app/store";
import { StatusBar } from "../../workbench/StatusBar";
import { BrowserClearData } from "./BrowserClearData";
import { resetBrowserClearDataForTests } from "./browserClearProgress";

const browserClearData = vi.fn();
const closeBrowser = vi.fn();
let clearDataEvent:
  ((payload: { cleared: boolean; error: string | null }) => void) | undefined;

vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(
    (
      _name: string,
      handler: (event: {
        payload: { cleared: boolean; error: string | null };
      }) => void,
    ) => {
      clearDataEvent = (payload) => handler({ payload });
      return Promise.resolve(vi.fn());
    },
  ),
}));

vi.mock("../../ports/nativeBrowserPort", () => ({
  browserClearData: () => browserClearData(),
  closeBrowser: (tabId: string) => closeBrowser(tabId),
}));

const original = useWorkbenchStore.getState();

beforeEach(() => {
  vi.clearAllMocks();
  resetBrowserClearDataForTests();
  clearDataEvent = undefined;
  closeBrowser.mockResolvedValue(undefined);
  useWorkbenchStore.setState({
    tabs: [
      {
        id: "browser-clear-test",
        kind: "browser",
        title: "Browser",
        position: 0,
        pinned: false,
        dirty: false,
      },
    ],
    activeTabId: "browser-clear-test",
    error: null,
    browserClearNotice: null,
  });
});

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
  useWorkbenchStore.setState({
    tabs: original.tabs,
    activeTabId: original.activeTabId,
    error: original.error,
    browserClearNotice: original.browserClearNotice,
    activateChat: original.activateChat,
  });
});

function confirmClear() {
  fireEvent.click(screen.getByRole("button", { name: "Clear browsing data" }));
  fireEvent.click(
    within(
      screen.getByRole("dialog", { name: "Clear browsing data?" }),
    ).getByRole("button", { name: "Clear browsing data" }),
  );
}

it("keeps browsing state unchanged when clearing is cancelled", async () => {
  vi.stubGlobal("confirm", () => Promise.resolve(false));
  browserClearData.mockResolvedValue({
    closedTabIds: ["browser-clear-test"],
    cleared: true,
    error: null,
  });
  render(<BrowserClearData />);
  const trigger = screen.getByRole("button", { name: "Clear browsing data" });
  trigger.focus();
  await act(async () => fireEvent.click(trigger));
  expect(useWorkbenchStore.getState().tabs.map((tab) => tab.id)).toEqual([
    "browser-clear-test",
  ]);
  await act(async () =>
    fireEvent.click(screen.getByRole("button", { name: "Cancel" })),
  );
  expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  expect(trigger).toHaveFocus();
  expect(useWorkbenchStore.getState().tabs.map((tab) => tab.id)).toEqual([
    "browser-clear-test",
  ]);
  expect(useWorkbenchStore.getState().browserClearNotice).toBeNull();
});

it("removes natively closed tabs without closing them twice", async () => {
  browserClearData.mockResolvedValue({
    closedTabIds: ["browser-clear-test"],
    cleared: true,
    error: null,
  });
  render(<BrowserClearData />);
  confirmClear();
  await waitFor(() =>
    expect(
      useWorkbenchStore
        .getState()
        .tabs.some((tab) => tab.id === "browser-clear-test"),
    ).toBe(false),
  );
  expect(closeBrowser).not.toHaveBeenCalled();
});

it("reports a partial clear failure after reconciling closed tabs", async () => {
  browserClearData.mockResolvedValue({
    closedTabIds: ["browser-clear-test"],
    cleared: false,
    error: "WebView2 clearing failed",
  });
  render(<BrowserClearData />);
  confirmClear();
  expect(await screen.findByRole("alert")).toHaveTextContent(
    "WebView2 clearing failed",
  );
  expect(useWorkbenchStore.getState().error).toBe("WebView2 clearing failed");
  expect(
    useWorkbenchStore
      .getState()
      .tabs.some((tab) => tab.id === "browser-clear-test"),
  ).toBe(false);
});

it("reports a pending clear failure when WebView2 finishes later", async () => {
  browserClearData.mockResolvedValue({
    closedTabIds: ["browser-clear-test"],
    cleared: false,
    pending: true,
    error:
      "Browser data is still clearing; new browser tabs are blocked until it finishes.",
  });
  render(<BrowserClearData />);
  confirmClear();
  expect(await screen.findByText(/still clearing/)).toBeInTheDocument();
  expect(
    screen.getByRole("button", { name: /Clearing browser data/ }),
  ).toBeDisabled();
  act(() =>
    clearDataEvent?.({ cleared: false, error: "WebView2 clearing failed" }),
  );
  expect(await screen.findByRole("alert")).toHaveTextContent(
    "WebView2 clearing failed",
  );
  expect(useWorkbenchStore.getState().error).toBe("WebView2 clearing failed");
  expect(
    screen.getByRole("button", { name: "Clear browsing data" }),
  ).toBeEnabled();
});

it("clears pending state when WebView2 succeeds later", async () => {
  browserClearData.mockResolvedValue({
    closedTabIds: ["browser-clear-test"],
    cleared: false,
    pending: true,
    error:
      "Browser data is still clearing; new browser tabs are blocked until it finishes.",
  });
  const view = render(<BrowserClearData />);
  confirmClear();
  expect(await screen.findByText(/still clearing/)).toBeInTheDocument();
  act(() => clearDataEvent?.({ cleared: true, error: null }));
  expect(await screen.findByRole("status")).toHaveTextContent(
    "Browsing data cleared.",
  );
  await waitFor(() =>
    expect(screen.queryByText(/still clearing/)).not.toBeInTheDocument(),
  );
  expect(
    screen.getByRole("button", { name: "Clear browsing data" }),
  ).toBeEnabled();
  expect(useWorkbenchStore.getState().error).toBeNull();
  view.unmount();
  render(<StatusBar showEditorSettings={false} />);
  expect(screen.getByRole("status")).toHaveTextContent(
    "Browsing data cleared.",
  );
});

it("keeps an early completion when it arrives before the command reply", async () => {
  let reply:
    | ((result: {
        closedTabIds: string[];
        cleared: boolean;
        pending: boolean;
        error: string;
      }) => void)
    | undefined;
  browserClearData.mockImplementation(
    () => new Promise((resolve) => (reply = resolve)),
  );
  render(<BrowserClearData />);
  confirmClear();
  await waitFor(() => expect(browserClearData).toHaveBeenCalledOnce());
  act(() => clearDataEvent?.({ cleared: true, error: null }));
  await act(async () => {
    reply?.({
      closedTabIds: ["browser-clear-test"],
      cleared: false,
      pending: true,
      error:
        "Browser data is still clearing; new browser tabs are blocked until it finishes.",
    });
  });
  expect(screen.queryByText(/still clearing/)).not.toBeInTheDocument();
  expect(
    screen.getByRole("button", { name: "Clear browsing data" }),
  ).toBeEnabled();
  expect(useWorkbenchStore.getState().error).toBeNull();
});

it("reports a late failure after the closed browser surface unmounts", async () => {
  browserClearData.mockResolvedValue({
    closedTabIds: ["browser-clear-test"],
    cleared: false,
    pending: true,
    error:
      "Browser data is still clearing; new browser tabs are blocked until it finishes.",
  });
  const view = render(<BrowserClearData />);
  confirmClear();
  await waitFor(() =>
    expect(useWorkbenchStore.getState().error).toContain("still clearing"),
  );
  view.unmount();
  act(() =>
    clearDataEvent?.({ cleared: false, error: "WebView2 clearing failed" }),
  );
  expect(useWorkbenchStore.getState().error).toBe("WebView2 clearing failed");
});

it("reconciles every closed tab before activating the next Chat", async () => {
  const activateChat = vi.fn(async () => undefined);
  useWorkbenchStore.setState({
    tabs: [
      {
        id: "browser-clear-test",
        kind: "browser",
        title: "Browser 1",
        position: 0,
        pinned: false,
        dirty: false,
      },
      {
        id: "chat-clear-test",
        kind: "chat",
        title: "Chat",
        position: 1,
        pinned: false,
        dirty: false,
      },
      {
        id: "browser-clear-test-2",
        kind: "browser",
        title: "Browser 2",
        position: 2,
        pinned: false,
        dirty: false,
      },
    ],
    activeTabId: "browser-clear-test",
    activateChat,
  });
  browserClearData.mockResolvedValue({
    closedTabIds: ["browser-clear-test", "browser-clear-test-2"],
    cleared: true,
    error: null,
  });
  render(<BrowserClearData />);
  confirmClear();
  await waitFor(() =>
    expect(useWorkbenchStore.getState().tabs.map((tab) => tab.id)).toEqual([
      "chat-clear-test",
    ]),
  );
  expect(activateChat).toHaveBeenCalledOnce();
  expect(activateChat).toHaveBeenCalledWith("chat-clear-test", [
    expect.objectContaining({ id: "chat-clear-test" }),
  ]);
  expect(closeBrowser).not.toHaveBeenCalled();
});

it("keeps a clear that outlives its control visible when the control comes back", async () => {
  let answer:
    | ((value: {
        closedTabIds: string[];
        cleared: boolean;
        error: null;
      }) => void)
    | undefined;
  browserClearData.mockReturnValue(
    new Promise((resolve) => {
      answer = resolve;
    }),
  );
  const first = render(<BrowserClearData />);
  confirmClear();
  await waitFor(() => expect(browserClearData).toHaveBeenCalledTimes(1));
  first.unmount();

  render(<BrowserClearData />);
  const button = screen.getByRole("button", { name: "Clearing browser data…" });
  expect(button).toBeDisabled();
  fireEvent.click(button);
  expect(browserClearData).toHaveBeenCalledTimes(1);

  await act(async () =>
    answer?.({
      closedTabIds: ["browser-clear-test"],
      cleared: true,
      error: null,
    }),
  );
  expect(
    await screen.findByRole("button", { name: "Clear browsing data" }),
  ).toBeEnabled();
  expect(screen.getByRole("status")).toHaveTextContent(
    "Browsing data cleared.",
  );
});

it("says plainly that nothing was cleared where WebView2 is unavailable", async () => {
  const message =
    "Clearing browsing data requires Windows WebView2. Nothing was cleared and no tabs were closed.";
  browserClearData.mockResolvedValue({
    closedTabIds: [],
    cleared: false,
    pending: false,
    error: message,
  });
  render(<BrowserClearData />);
  confirmClear();
  expect(await screen.findByRole("alert")).toHaveTextContent(message);
  expect(
    useWorkbenchStore
      .getState()
      .tabs.some((tab) => tab.id === "browser-clear-test"),
  ).toBe(true);
  expect(
    screen.getByRole("button", { name: "Clear browsing data" }),
  ).toBeEnabled();
});
