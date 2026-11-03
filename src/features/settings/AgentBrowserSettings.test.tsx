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
  browserPermissionStatus,
  browserSiteSettingReset,
} from "../../ports/nativeBrowserPort";
import {
  browserDownloads,
  browserCancelDownload,
  browserShowDownloadInFolder,
} from "../../ports/nativeBrowserDownloadsPort";

vi.mock("../../ports/nativeBrowserPreferencesPort", () => ({
  browserPreferences: vi.fn(async () => ({ showFullUrl: false })),
  browserSetPreferences: vi.fn(async () => undefined),
}));
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
  browserPermissionStatus: vi.fn(async (tabId: string) => ({
    tabId,
    pending: [],
    decisions: [],
  })),
  browserPermissionRevoke: vi.fn(),
  browserSiteSettingReset: vi.fn(),
  browserPermissionResolve: vi.fn(),
  browserClearData: vi.fn(),
}));
vi.mock("../../ports/nativeBrowserSiteSettingsPort", () => ({
  browserSiteSettings: vi.fn(async () => ({
    settings: [
      { origin: "https://saved.example", kind: "camera", decision: "allow" },
    ],
    staleSites: [],
  })),
}));
vi.mock("../../ports/nativeBrowserDownloadsPort", () => ({
  browserDownloads: vi.fn(async () => []),
  browserCancelDownload: vi.fn(),
  browserShowDownloadInFolder: vi.fn(),
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
  await import("../browser/BrowserDownloadsPanel");
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
  expect(await screen.findByText("No downloads yet")).toBeVisible();
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

it("resets a saved site permission even when no browser tab is live", async () => {
  vi.mocked(browserLiveTabs).mockResolvedValueOnce([]);
  vi.mocked(browserSiteSettingReset).mockResolvedValueOnce({
    settings: [],
    staleSites: [],
  });
  render(<AgentBrowserSettings />);
  await screen.findByText("No live browser tabs");
  fireEvent.click(
    await screen.findByRole("button", {
      name: "Reset Camera for https://saved.example",
    }),
  );
  expect(
    await screen.findByText("No site permissions saved yet."),
  ).toBeVisible();
  expect(screen.queryByText("https://saved.example")).toBeNull();
});

it("manages live-tab downloads through the shared lazy downloads panel", async () => {
  vi.mocked(browserDownloads).mockResolvedValueOnce([
    {
      tabId: "browser-b",
      id: 1,
      source: "https://example.com/report.pdf",
      fileName: "report.pdf",
      state: "completed",
      receivedBytes: 10,
      totalBytes: 10,
    },
    {
      tabId: "browser-b",
      id: 2,
      source: "https://example.com/archive.zip",
      fileName: "archive.zip",
      state: "downloading",
      receivedBytes: 5,
      totalBytes: 10,
    },
  ]);
  vi.mocked(browserCancelDownload).mockResolvedValueOnce(undefined);
  vi.mocked(browserShowDownloadInFolder).mockResolvedValueOnce(undefined);
  render(<AgentBrowserSettings />);
  fireEvent.click(
    await screen.findByRole("button", { name: "Manage downloads" }),
  );
  expect(
    await screen.findByRole("progressbar", {
      name: "archive.zip download progress",
    }),
  ).toHaveAttribute("value", "5");
  expect(browserCancelDownload).not.toHaveBeenCalled();
  expect(browserShowDownloadInFolder).not.toHaveBeenCalled();
  fireEvent.click(screen.getByRole("button", { name: "Cancel archive.zip" }));
  expect(browserCancelDownload).toHaveBeenCalledWith("browser-b", 2);
  fireEvent.click(
    screen.getByRole("button", { name: "Show report.pdf in folder" }),
  );
  expect(browserShowDownloadInFolder).toHaveBeenCalledWith("browser-b", 1);
  fireEvent.click(screen.getByRole("button", { name: "Manage downloads" }));
  expect(screen.queryByRole("listitem")).toBeNull();
});

it("reports closed-tab history and download management as unavailable rather than inventing persistent records", async () => {
  vi.mocked(browserLiveTabs).mockResolvedValueOnce([]);
  render(<AgentBrowserSettings />);
  await screen.findByText("No live browser tabs");
  expect(
    screen.getByText(
      /Closed-tab history and download management are not available yet/,
    ),
  ).toBeVisible();
  expect(
    screen.queryByRole("button", { name: "Manage browsing history" }),
  ).toBeNull();
  expect(screen.queryByRole("button", { name: "Manage downloads" })).toBeNull();
  expect(browserHistory).not.toHaveBeenCalled();
  expect(browserDownloads).not.toHaveBeenCalled();
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

it("opens site permissions directly when Site settings requests the originating tab", async () => {
  useAgentSettingsState.getState().open("Browser", "browser-b", true);
  render(<AgentBrowserSettings />);
  await waitFor(() =>
    expect(browserPermissionStatus).toHaveBeenCalledWith("browser-b"),
  );
  expect(
    screen.getByRole("button", { name: "Manage site permissions" }),
  ).toHaveAttribute("aria-expanded", "true");
});
