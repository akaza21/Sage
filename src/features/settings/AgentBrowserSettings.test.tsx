import {
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { useWorkbenchStore } from "../../app/store";
import { useAgentSettingsState } from "./agentSettingsState";
import { AgentBrowserSettings } from "./AgentBrowserSettings";
import { isTauriRuntime } from "../../ports/nativeInvoke";
import {
  browserHistory,
  browserLiveTabs,
  browserDownloads,
  browserPermissionStatus,
} from "../../ports/nativeBrowserPort";

vi.mock("../../ports/nativeInvoke", () => ({
  isTauriRuntime: vi.fn(() => true),
}));
vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(async () => vi.fn()),
}));
vi.mock("../../ports/nativeBrowserPort", () => ({
  browserLiveTabs: vi.fn(async () => ["browser-a", "browser-b"]),
  closeBrowser: vi.fn(async () => undefined),
  browserHistory: vi.fn(async () => ["https://example.com/visited"]),
  browserDownloads: vi.fn(async () => []),
  browserPermissionStatus: vi.fn(async (tabId: string) => ({
    tabId,
    pending: [],
    decisions: [],
  })),
  browserPermissionRevoke: vi.fn(),
  browserPermissionResolve: vi.fn(),
  browserCancelDownload: vi.fn(),
  browserShowDownloadInFolder: vi.fn(),
  browserClearData: vi.fn(),
}));
const initial = useWorkbenchStore.getState();
beforeEach(() => {
  vi.clearAllMocks();
  vi.mocked(isTauriRuntime).mockReturnValue(true);
  useAgentSettingsState.getState().open("Browser", "browser-b");
  useWorkbenchStore.setState({
    tabs: [
      {
        id: "browser-a",
        kind: "browser",
        title: "First browser",
        position: 0,
        dirty: false,
        pinned: false,
      },
      {
        id: "browser-b",
        kind: "browser",
        title: "Second browser",
        position: 1,
        dirty: false,
        pinned: false,
      },
    ],
  });
});
afterEach(() => {
  cleanup();
  useWorkbenchStore.setState(initial, true);
  useAgentSettingsState.getState().close();
});

it("scopes native history, permissions and downloads to the requested live browser tab", async () => {
  render(<AgentBrowserSettings />);
  await waitFor(() =>
    expect(screen.getByRole("combobox", { name: "Browser tab" })).toHaveValue(
      "browser-b",
    ),
  );
  fireEvent.click(
    screen.getByRole("button", { name: "Manage site permissions" }),
  );
  await waitFor(() =>
    expect(browserPermissionStatus).toHaveBeenCalledWith("browser-b"),
  );
  fireEvent.click(
    screen.getByRole("button", { name: "Manage browsing history" }),
  );
  expect(await screen.findByText("https://example.com/visited")).toBeVisible();
  expect(browserHistory).toHaveBeenCalledWith("browser-b");
  fireEvent.click(screen.getByRole("button", { name: "Manage downloads" }));
  await waitFor(() =>
    expect(browserDownloads).toHaveBeenCalledWith("browser-b"),
  );
  expect(screen.getByText("No downloads yet.")).toBeVisible();
  fireEvent.change(screen.getByRole("combobox", { name: "Browser tab" }), {
    target: { value: "browser-a" },
  });
  expect(screen.queryByText("https://example.com/visited")).toBeNull();
});

it("does not fake browser settings outside the native runtime", () => {
  vi.mocked(isTauriRuntime).mockReturnValue(false);
  render(<AgentBrowserSettings />);
  expect(screen.getByText(/available in the desktop app/)).toBeVisible();
  expect(browserLiveTabs).not.toHaveBeenCalled();
  expect(
    screen.queryByRole("button", { name: "Clear browsing data" }),
  ).toBeNull();
});

it("shows native history failure rather than claiming history is empty", async () => {
  vi.mocked(browserHistory).mockRejectedValueOnce(
    new Error("History unavailable"),
  );
  render(<AgentBrowserSettings />);
  fireEvent.click(
    await screen.findByRole("button", { name: "Manage browsing history" }),
  );
  expect(await screen.findByRole("alert")).toHaveTextContent(
    "History unavailable",
  );
  expect(screen.queryByText("No history for this tab yet.")).toBeNull();
});

it("does not retain live-tab controls after the native browser data has been cleared", async () => {
  render(<AgentBrowserSettings />);
  await screen.findByRole("button", { name: "Manage browsing history" });
  vi.mocked(browserLiveTabs).mockResolvedValueOnce([]);
  const { act } = await import("@testing-library/react");
  act(() =>
    useWorkbenchStore.setState({
      browserClearNotice: "Browsing data cleared.",
    }),
  );
  await waitFor(() =>
    expect(
      screen.getByRole("combobox", { name: "Browser tab" }),
    ).toBeDisabled(),
  );
  expect(
    screen.queryByRole("button", { name: "Manage browsing history" }),
  ).toBeNull();
});
