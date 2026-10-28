import {
  act,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { Activity } from "react";
import { listen } from "@tauri-apps/api/event";
import { expect, it, vi } from "vitest";

import type { BrowserDownload } from "../../ports/nativeBrowserPort";
import { BrowserDownloads } from "./BrowserDownloads";
import { progressLabel } from "./browserDownloadLabel";

const browserDownloads = vi.fn(
  async (_tabId: string): Promise<BrowserDownload[]> => [],
);
const showInFolder = vi.fn(async (_tabId: string, _id: number) => undefined);
vi.mock("../../ports/nativeInvoke", () => ({ isTauriRuntime: () => true }));
vi.mock("../../ports/nativeBrowserPort", () => ({
  browserDownloads: (tabId: string) => browserDownloads(tabId),
  browserCancelDownload: vi.fn(async () => undefined),
  browserShowDownloadInFolder: (tabId: string, id: number) =>
    showInFolder(tabId, id),
}));
vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(async () => () => undefined),
}));

const download: BrowserDownload = {
  tabId: "browser-1",
  id: 1,
  source: "https://example.com",
  fileName: "report.pdf",
  state: "downloading",
  receivedBytes: 5,
  totalBytes: 10,
};

it("labels bounded progress and all terminal download states", () => {
  expect(progressLabel(download)).toBe("50%");
  expect(progressLabel({ ...download, receivedBytes: 20 })).toBe("100%");
  expect(progressLabel({ ...download, totalBytes: null })).toBe("Downloading");
  expect(progressLabel({ ...download, state: "completed" })).toBe("Downloaded");
  expect(progressLabel({ ...download, state: "interrupted" })).toBe(
    "Download failed",
  );
  expect(progressLabel({ ...download, state: "canceled" })).toBe("Canceled");
});

it("reveals a completed download's folder only after an explicit click", async () => {
  browserDownloads.mockResolvedValueOnce([{ ...download, state: "completed" }]);
  render(<BrowserDownloads tabId="browser-1" open />);
  await screen.findByText("report.pdf");
  expect(showInFolder).not.toHaveBeenCalled();
  fireEvent.click(
    screen.getByRole("button", { name: "Show report.pdf in folder" }),
  );
  await waitFor(() =>
    expect(showInFolder).toHaveBeenCalledWith("browser-1", 1),
  );
});

it("disposes late downloads registration after Activity sleeps without reading hidden state", async () => {
  browserDownloads.mockClear();
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
      <BrowserDownloads tabId="browser-1" open />
    </Activity>,
  );
  await waitFor(() => expect(finish).toBeDefined());
  view.rerender(
    <Activity mode="hidden">
      <BrowserDownloads tabId="browser-1" open />
    </Activity>,
  );
  await act(async () => finish?.(dispose));
  expect(dispose).toHaveBeenCalledOnce();
  expect(browserDownloads).not.toHaveBeenCalled();
  view.unmount();
});
