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
import type { BrowserDevice } from "../../ports/nativeBrowserPort";
import { BrowserSurface } from "./BrowserSurface";

const browserStatus = vi.fn();
const browserSetDevice = vi.fn(
  async (_tabId: string, _device: BrowserDevice | null) => undefined,
);
vi.mock("../../ports/nativeInvoke", () => ({ isTauriRuntime: () => true }));
vi.mock("../../ports/nativeBrowserPort", () => ({
  openBrowser: vi.fn(async () => "browser-new"),
  closeBrowser: vi.fn(async () => undefined),
  browserPopupPending: vi.fn(async () => []),
  browserStatus: (tabId: string) => browserStatus(tabId),
  browserSetDevice: (tabId: string, device: BrowserDevice | null) =>
    browserSetDevice(tabId, device),
  browserPageContext: vi.fn(),
  browserZoom: vi.fn(async () => undefined),
  browserFind: vi.fn(async () => undefined),
  browserOpenExternal: vi.fn(async () => undefined),
  browserPrint: vi.fn(async () => undefined),
  browserCaptureScreenshot: vi.fn(),
  browserSuggestedSites: vi.fn(async () => []),
  browserHistory: vi.fn(async () => []),
  attachBrowserTab: vi.fn(),
  detachBrowserTab: vi.fn(async () => undefined),
  stopBrowserTab: vi.fn(async () => undefined),
  browserAttachmentStatus: vi.fn(async () => null),
  browserPermissionStatus: vi.fn(async (tabId: string) => ({
    tabId,
    origin: "unavailable",
    pending: [],
    decisions: [],
  })),
  browserControl: vi.fn(),
  browserAgentControl: vi.fn(async () => null),
  browserNavigate: vi.fn(async () => undefined),
  hideBrowser: vi.fn(async () => undefined),
  coverBrowser: vi.fn(async () => undefined),
  positionBrowser: vi.fn(async () => undefined),
}));
vi.mock("../../ports/nativeBrowserPreferencesPort", () => ({
  browserPreferences: vi.fn(async () => ({ showFullUrl: false })),
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

/** The page frame is 878 × 916 before the device toolbar takes 36 px of it,
 * which leaves an 842 × 862 device after the 18 px gutters. */
function frameRect(this: HTMLElement) {
  const [width, height] = this.classList.contains("browser-surface__frame")
    ? [878, 916]
    : [0, 0];
  return {
    x: 0,
    y: 0,
    left: 0,
    top: 0,
    width,
    height,
    right: width,
    bottom: height,
  } as DOMRect;
}

beforeEach(() => {
  vi.clearAllMocks();
  vi.stubGlobal(
    "ResizeObserver",
    class {
      observe() {}
      disconnect() {}
    },
  );
  vi.spyOn(HTMLElement.prototype, "getBoundingClientRect").mockImplementation(
    frameRect,
  );
  HTMLElement.prototype.setPointerCapture = vi.fn();
  browserStatus.mockResolvedValue({
    tabId: tab.id,
    url: "https://example.com/",
    revision: "1",
    loading: false,
    canGoBack: false,
    canGoForward: false,
    zoomPercent: 100,
    device: null,
  });
});

afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
});

async function openDevice() {
  render(<BrowserSurface tab={tab} />);
  await screen.findByLabelText("Secure connection");
  fireEvent.click(screen.getByRole("button", { name: "More options" }));
  fireEvent.click(
    screen.getByRole("menuitem", { name: "Show device toolbar" }),
  );
  return screen.findByRole("toolbar", { name: "Device toolbar" });
}

it("opens Responsive at the pane's size and draws the page in a gutter", async () => {
  await openDevice();
  await waitFor(() =>
    expect(browserSetDevice).toHaveBeenLastCalledWith(tab.id, {
      preset: "responsive",
      width: 842,
      height: 862,
      scalePercent: 100,
    }),
  );
  expect(screen.getByRole("spinbutton", { name: "Width" })).toHaveValue(842);
  expect(screen.getByRole("spinbutton", { name: "Height" })).toHaveValue(862);
  expect(screen.getByText("100%")).toBeInTheDocument();
  const frame = document.querySelector(".browser-surface__frame")!;
  expect(frame).toHaveClass("browser-surface__frame--device");
  expect(
    (frame as HTMLElement).style.getPropertyValue("--browser-device-width"),
  ).toBe("842px");
  expect(
    document.querySelectorAll(
      ".browser-surface__viewport > .browser-device-handle",
    ),
  ).toHaveLength(5);
  fireEvent.click(screen.getByRole("button", { name: "More options" }));
  expect(
    screen.getByRole("menuitem", { name: "Hide device toolbar" }),
  ).toBeEnabled();
});

it("applies a fixed preset read-only, rotates it, and drops the handles", async () => {
  await openDevice();
  fireEvent.change(screen.getByRole("combobox", { name: "Dimensions:" }), {
    target: { value: "iphone-se" },
  });
  await waitFor(() =>
    expect(browserSetDevice).toHaveBeenLastCalledWith(tab.id, {
      preset: "iphone-se",
      width: 375,
      height: 667,
      scalePercent: 100,
    }),
  );
  expect(screen.getByRole("spinbutton", { name: "Width" })).toHaveAttribute(
    "readonly",
  );
  expect(document.querySelector(".browser-device-handle")).toBeNull();
  fireEvent.click(screen.getByRole("button", { name: "Rotate" }));
  await waitFor(() =>
    expect(browserSetDevice).toHaveBeenLastCalledWith(tab.id, {
      preset: "iphone-se",
      width: 667,
      height: 375,
      scalePercent: 100,
    }),
  );
});

it("scales a device larger than the pane down to fit", async () => {
  await openDevice();
  fireEvent.change(screen.getByRole("combobox", { name: "Dimensions:" }), {
    target: { value: "ipad-air" },
  });
  // 1180 px tall in 898 px of room: drawn at 76%.
  await waitFor(() =>
    expect(browserSetDevice).toHaveBeenLastCalledWith(tab.id, {
      preset: "ipad-air",
      width: 820,
      height: 1180,
      scalePercent: 76,
    }),
  );
  expect(screen.getByText("76%")).toBeInTheDocument();
});

it("commits a typed Responsive width within the admitted bounds", async () => {
  await openDevice();
  const width = screen.getByRole("spinbutton", { name: "Width" });
  fireEvent.change(width, { target: { value: "20" } });
  fireEvent.blur(width);
  await waitFor(() =>
    expect(browserSetDevice).toHaveBeenLastCalledWith(
      tab.id,
      expect.objectContaining({ width: 50, height: 862 }),
    ),
  );
  fireEvent.change(width, { target: { value: "400" } });
  fireEvent.keyDown(width, { key: "Enter" });
  await waitFor(() =>
    expect(browserSetDevice).toHaveBeenLastCalledWith(
      tab.id,
      expect.objectContaining({ width: 400 }),
    ),
  );
});

it("resizes a centred Responsive device by twice a side handle's travel", async () => {
  await openDevice();
  const right = document.querySelector(".browser-device-handle--right")!;
  fireEvent.pointerDown(right, { pointerId: 1, clientX: 100, clientY: 100 });
  fireEvent.pointerMove(right, { pointerId: 1, clientX: 90, clientY: 100 });
  fireEvent.pointerUp(right, { pointerId: 1 });
  await waitFor(() =>
    expect(browserSetDevice).toHaveBeenLastCalledWith(
      tab.id,
      expect.objectContaining({ width: 822, height: 862 }),
    ),
  );
  const bottom = document.querySelector(".browser-device-handle--bottom")!;
  fireEvent.pointerDown(bottom, { pointerId: 1, clientX: 0, clientY: 100 });
  fireEvent.pointerMove(bottom, { pointerId: 1, clientX: 0, clientY: 70 });
  await waitFor(() =>
    expect(browserSetDevice).toHaveBeenLastCalledWith(
      tab.id,
      expect.objectContaining({ width: 822, height: 832 }),
    ),
  );
});

it("sends only the latest change while an update is in flight", async () => {
  let release = () => {};
  browserSetDevice.mockImplementationOnce(
    () =>
      new Promise<undefined>((resolve) => (release = () => resolve(undefined))),
  );
  await openDevice();
  await waitFor(() => expect(browserSetDevice).toHaveBeenCalledTimes(1));
  const width = screen.getByRole("spinbutton", { name: "Width" });
  for (const value of ["300", "310", "320"]) {
    fireEvent.change(width, { target: { value } });
    fireEvent.blur(width);
  }
  expect(browserSetDevice).toHaveBeenCalledTimes(1);
  await act(async () => release());
  await waitFor(() => expect(browserSetDevice).toHaveBeenCalledTimes(2));
  expect(browserSetDevice).toHaveBeenLastCalledWith(
    tab.id,
    expect.objectContaining({ width: 320 }),
  );
});

it("closes the toolbar and resets the tab", async () => {
  await openDevice();
  fireEvent.click(screen.getByRole("button", { name: "Close device toolbar" }));
  await waitFor(() =>
    expect(browserSetDevice).toHaveBeenLastCalledWith(tab.id, null),
  );
  expect(screen.queryByRole("toolbar", { name: "Device toolbar" })).toBeNull();
  expect(document.querySelector(".browser-surface__frame")).not.toHaveClass(
    "browser-surface__frame--device",
  );
});

it("restores the tab's emulation from Rust when the surface remounts", async () => {
  browserStatus.mockResolvedValue({
    tabId: tab.id,
    url: "https://example.com/",
    revision: "1",
    loading: false,
    canGoBack: false,
    canGoForward: false,
    zoomPercent: 100,
    device: { preset: "pixel-7", width: 915, height: 412, scalePercent: 92 },
  });
  render(<BrowserSurface tab={tab} />);
  await screen.findByRole("toolbar", { name: "Device toolbar" });
  expect(screen.getByRole("combobox", { name: "Dimensions:" })).toHaveValue(
    "pixel-7",
  );
  expect(screen.getByRole("spinbutton", { name: "Width" })).toHaveValue(915);
});

it("reports a failed apply and closes the toolbar Rust already reset", async () => {
  browserSetDevice.mockRejectedValueOnce("device toolbar could not update");
  await openDevice();
  expect(await screen.findByRole("alert")).toHaveTextContent(
    "device toolbar could not update",
  );
  await waitFor(() =>
    expect(
      screen.queryByRole("toolbar", { name: "Device toolbar" }),
    ).toBeNull(),
  );
});

it("keeps the menu item off where Rust says emulation is unavailable", async () => {
  browserStatus.mockResolvedValue({
    tabId: tab.id,
    url: "https://example.com/",
    revision: "1",
    loading: false,
    canGoBack: false,
    canGoForward: false,
    zoomPercent: 100,
    deviceUnavailableReason: "Device toolbar is unavailable on this platform",
  });
  render(<BrowserSurface tab={tab} />);
  await screen.findByLabelText("Secure connection");
  fireEvent.click(screen.getByRole("button", { name: "More options" }));
  const item = screen.getByRole("menuitem", { name: "Show device toolbar" });
  expect(item).toBeDisabled();
  expect(item).toHaveAttribute(
    "title",
    "Device toolbar is unavailable on this platform",
  );
});

it("never carries one tab's device over to the next tab in the same pane", async () => {
  const other: WorkbenchTab = { ...tab, id: "browser-2" };
  browserStatus.mockImplementation(async (tabId: string) => ({
    tabId,
    url: "https://example.com/",
    revision: "1",
    loading: false,
    canGoBack: false,
    canGoForward: false,
    zoomPercent: 100,
    device:
      tabId === tab.id
        ? { preset: "pixel-7", width: 412, height: 915, scalePercent: 94 }
        : null,
  }));
  const { rerender } = render(<BrowserSurface tab={tab} />);
  await screen.findByRole("toolbar", { name: "Device toolbar" });
  rerender(<BrowserSurface tab={other} />);
  await waitFor(() => expect(browserStatus).toHaveBeenCalledWith(other.id));
  await screen.findByLabelText("Secure connection");
  expect(screen.queryByRole("toolbar", { name: "Device toolbar" })).toBeNull();
  expect(browserSetDevice).not.toHaveBeenCalledWith(
    other.id,
    expect.anything(),
  );
  expect(document.querySelector(".browser-surface__frame--device")).toBeNull();
});
