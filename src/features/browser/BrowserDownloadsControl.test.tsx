import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { Activity, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { afterEach, beforeEach, expect, it, vi } from "vitest";

import type { BrowserDownload } from "../../ports/nativeBrowserDownloadsPort";
import { BrowserDownloadsControl } from "./BrowserDownloadsControl";
import BrowserDownloadsPanel from "./BrowserDownloadsPanel";
import { activeDownloadProgress } from "./useBrowserDownloads";

const browserDownloads = vi.fn(
  async (_tabId: string): Promise<BrowserDownload[]> => [],
);
const cancelDownload = vi.fn(async (_tabId: string, _id: number) => undefined);
const showInFolder = vi.fn(async (_tabId: string, _id: number) => undefined);
vi.mock("../../ports/nativeInvoke", () => ({ isTauriRuntime: () => true }));
vi.mock("../../ports/nativeBrowserDownloadsPort", () => ({
  browserDownloads: (tabId: string) => browserDownloads(tabId),
  browserCancelDownload: (tabId: string, id: number) =>
    cancelDownload(tabId, id),
  browserShowDownloadInFolder: (tabId: string, id: number) =>
    showInFolder(tabId, id),
}));
vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(async () => () => undefined),
}));

const download: BrowserDownload = {
  tabId: "browser-1",
  id: 1,
  source: "https://example.com/files/report.pdf",
  fileName: "report.pdf",
  state: "downloading",
  receivedBytes: 5,
  totalBytes: 10,
};

function Harness({ onPaneKeyDown }: { onPaneKeyDown?: () => void }) {
  const [open, setOpen] = useState(false);
  return (
    <div onKeyDown={onPaneKeyDown}>
      <BrowserDownloadsControl
        tabId="browser-1"
        open={open}
        onOpenChange={setOpen}
      />
      <p>Outside</p>
    </div>
  );
}

beforeEach(() => {
  vi.clearAllMocks();
});

afterEach(cleanup);

it.each(["registration", "read"])(
  "keeps the newest download state when an event arrives during %s",
  async (gap) => {
    let register!: () => void;
    let deliver!: (downloads: BrowserDownload[]) => void;
    vi.mocked(listen).mockImplementationOnce(
      (_name, handler) =>
        new Promise((resolve) => {
          register = () => resolve(() => undefined);
          deliver = (downloads) =>
            handler({
              event: "browser:downloads",
              id: 1,
              payload: { tabId: "browser-1", downloads },
            });
        }),
    );
    let answer!: (downloads: BrowserDownload[]) => void;
    browserDownloads.mockReturnValueOnce(
      new Promise((resolve) => {
        answer = resolve;
      }),
    );
    render(<Harness />);
    await waitFor(() => expect(register).toBeDefined());
    const completed = { ...download, state: "completed" as const };
    if (gap === "registration") act(() => deliver([download]));
    await act(async () => register());
    if (gap === "read") act(() => deliver([completed]));
    await act(async () => answer([gap === "read" ? download : completed]));
    const button = screen.getByRole("button", { name: "Downloads" });
    fireEvent.click(button);
    expect(await screen.findByText("Downloaded · example.com")).toBeVisible();
  },
);

it("disposes late downloads registration after Activity sleeps without reading hidden state", async () => {
  let finish: ((dispose: () => void) => void) | undefined;
  const dispose = vi.fn();
  vi.mocked(listen).mockImplementationOnce(
    () =>
      new Promise((resolve) => {
        finish = resolve;
      }),
  );
  const view = render(
    <Activity mode="visible">
      <Harness />
    </Activity>,
  );
  await waitFor(() => expect(finish).toBeDefined());
  view.rerender(
    <Activity mode="hidden">
      <Harness />
    </Activity>,
  );
  await act(async () => finish?.(dispose));
  expect(dispose).toHaveBeenCalledOnce();
  expect(browserDownloads).not.toHaveBeenCalled();
  view.unmount();
});

it("labels bounded progress and all terminal download states", () => {
  const progressLabel = (item: BrowserDownload) => {
    const view = render(
      <BrowserDownloadsPanel tabId="browser-1" downloads={[item]} />,
    );
    const label = view.container
      .querySelector(".browser-downloads__status")!
      .textContent!.split(" · ")[0];
    view.unmount();
    return label;
  };
  expect(progressLabel(download)).toBe("50%");
  expect(progressLabel({ ...download, receivedBytes: 20 })).toBe("100%");
  expect(progressLabel({ ...download, totalBytes: null })).toBe("Downloading");
  expect(progressLabel({ ...download, state: "completed" })).toBe("Downloaded");
  expect(progressLabel({ ...download, state: "interrupted" })).toBe(
    "Download failed",
  );
  expect(progressLabel({ ...download, state: "canceled" })).toBe("Canceled");
});

it("sums only the active downloads with a known size into the ring", () => {
  expect(activeDownloadProgress([])).toBeNull();
  expect(
    activeDownloadProgress([
      download,
      { ...download, id: 2, receivedBytes: 30, totalBytes: 30 },
      { ...download, id: 3, totalBytes: null },
      { ...download, id: 4, state: "completed", receivedBytes: 0 },
    ]),
  ).toBeCloseTo(35 / 40);
});

it("shows the destination timeout and recovery guidance in the failed row", () => {
  const error =
    "Download destination I/O timed out. Downloaded bytes may be recovered from the private download folder";
  render(
    <BrowserDownloadsPanel
      tabId="browser-1"
      downloads={[{ ...download, state: "error", error }]}
    />,
  );
  expect(screen.getByText(`${error} · example.com`)).toBeInTheDocument();
  expect(screen.getByTitle(error)).toBeInTheDocument();
  expect(
    screen.queryByRole("button", { name: "Cancel report.pdf" }),
  ).toBeNull();
});

it("shows active progress on the toolbar button while the popover is shut", async () => {
  browserDownloads.mockResolvedValueOnce([download]);
  const view = render(<Harness />);
  const button = await screen.findByRole("button", {
    name: "Downloads, 1 active",
  });
  expect(button).toHaveAttribute("aria-expanded", "false");
  expect(button.style.getPropertyValue("--browser-download-progress")).toBe(
    "50%",
  );
  expect(view.container.querySelector("[data-active]")).not.toBeNull();
  expect(view.container.querySelector("[data-indeterminate]")).toBeNull();
  expect(screen.queryByRole("dialog")).toBeNull();
});

it("keeps saving downloads active without offering cancellation or a completed folder", async () => {
  browserDownloads.mockResolvedValueOnce([
    { ...download, state: "publishing" },
  ]);
  render(<Harness />);
  fireEvent.click(
    await screen.findByRole("button", { name: "Downloads, 1 active" }),
  );
  expect(await screen.findByText("Saving · example.com")).toBeInTheDocument();
  expect(
    screen.queryByRole("button", { name: "Cancel report.pdf" }),
  ).toBeNull();
  expect(
    screen.queryByRole("button", { name: "Show report.pdf in folder" }),
  ).toBeNull();
});

it("opens a popover listing downloads newest first instead of a bar", async () => {
  browserDownloads.mockResolvedValueOnce([
    { ...download, state: "completed", fileName: "old.zip" },
    { ...download, id: 2 },
  ]);
  render(<Harness />);
  fireEvent.click(
    await screen.findByRole("button", { name: "Downloads, 1 active" }),
  );
  const dialog = screen.getByRole("dialog", { name: "Downloads" });
  const items = await screen.findAllByRole("listitem");
  expect(items.map((item) => item.textContent)).toEqual([
    expect.stringContaining("report.pdf50% · example.com"),
    expect.stringContaining("old.zipDownloaded · example.com"),
  ]);
  expect(dialog).toHaveFocus();
  expect(
    screen.getByRole("progressbar", { name: "report.pdf download progress" }),
  ).toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "Cancel report.pdf" }));
  await waitFor(() =>
    expect(cancelDownload).toHaveBeenCalledWith("browser-1", 2),
  );
  expect(showInFolder).not.toHaveBeenCalled();
  fireEvent.click(
    screen.getByRole("button", { name: "Show old.zip in folder" }),
  );
  await waitFor(() =>
    expect(showInFolder).toHaveBeenCalledWith("browser-1", 1),
  );
});

it("says there are no downloads yet in an empty popover", async () => {
  render(<Harness />);
  fireEvent.click(screen.getByRole("button", { name: "Downloads" }));
  expect(await screen.findByText("No downloads yet")).toBeInTheDocument();
});

it("closes only the popover on Escape and returns focus to the button", async () => {
  const onPaneKeyDown = vi.fn();
  render(<Harness onPaneKeyDown={onPaneKeyDown} />);
  const button = screen.getByRole("button", { name: "Downloads" });
  fireEvent.click(button);
  const dialog = screen.getByRole("dialog", { name: "Downloads" });
  await screen.findByText("No downloads yet");
  fireEvent.keyDown(dialog, { key: "Escape" });
  expect(screen.queryByRole("dialog")).toBeNull();
  expect(button).toHaveAttribute("aria-expanded", "false");
  expect(button).toHaveFocus();
  expect(onPaneKeyDown).not.toHaveBeenCalled();
  fireEvent.keyDown(button, { key: "Escape" });
  expect(onPaneKeyDown).toHaveBeenCalledTimes(1);
});

it("closes on a pointer outside but not on one inside", async () => {
  render(<Harness />);
  fireEvent.click(screen.getByRole("button", { name: "Downloads" }));
  fireEvent.pointerDown(await screen.findByText("No downloads yet"));
  expect(screen.getByRole("dialog")).toBeInTheDocument();
  fireEvent.pointerDown(screen.getByText("Outside"));
  expect(screen.queryByRole("dialog")).toBeNull();
});
