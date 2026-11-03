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
import type { WorkbenchTab } from "../../domain/workbench";
import type { BrowserDevice } from "../../ports/nativeBrowserPort";
import { BrowserSurface } from "./BrowserSurface";

const browserStatus = vi.fn();
const setDeviceInvoke = vi.fn(
  async (_tabId: string, _device: BrowserDevice | null) => undefined,
);
vi.mock("../../ports/nativeInvoke", () => ({
  isTauriRuntime: () => true,
  invokeNative: async (
    command: string,
    args: { tabId: string; device: BrowserDevice | null },
  ) => {
    if (command === "browser_set_device")
      return setDeviceInvoke(args.tabId, args.device);
    throw new Error(`Unexpected native command: ${command}`);
  },
}));
vi.mock("../../ports/nativeBrowserPort", () => ({
  openBrowser: vi.fn(async () => "browser-new"),
  closeBrowser: vi.fn(async () => undefined),
  browserPopupPending: vi.fn(async () => []),
  browserStatus: (tabId: string) => browserStatus(tabId),
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

/** The page frame is 878 × 916 until the device toolbar's row takes 36 px
 * of it, which leaves an 842 × 862 device after the 18 px gutters. */
function frameRect(this: HTMLElement) {
  const toolbar = document.querySelector(".browser-surface__device-slot");
  const [width, height] = this.classList.contains("browser-surface__frame")
    ? [878, toolbar ? 880 : 916]
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
    expect(setDeviceInvoke).toHaveBeenLastCalledWith(tab.id, {
      preset: "responsive",
      width: 842,
      height: 862,
      scalePercent: 100,
    }),
  );
  expect(screen.getByRole("spinbutton", { name: "Width" })).toHaveValue(842);
  expect(screen.getByRole("spinbutton", { name: "Height" })).toHaveValue(862);
  expect(
    within(screen.getByRole("toolbar", { name: "Device toolbar" })).getByText(
      "100%",
    ),
  ).toBeVisible();
  const frame = document.querySelector(".browser-surface__frame")!;
  expect(frame).toHaveAttribute("data-device");
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
    expect(setDeviceInvoke).toHaveBeenLastCalledWith(tab.id, {
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
    expect(setDeviceInvoke).toHaveBeenLastCalledWith(tab.id, {
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
  // 1180 px tall in 862 px of room: drawn at 73%.
  await waitFor(() =>
    expect(setDeviceInvoke).toHaveBeenLastCalledWith(tab.id, {
      preset: "ipad-air",
      width: 820,
      height: 1180,
      scalePercent: 73,
    }),
  );
  expect(screen.getByText("73%")).toBeInTheDocument();
});

it("commits a typed Responsive width within the admitted bounds", async () => {
  await openDevice();
  const width = screen.getByRole("spinbutton", { name: "Width" });
  fireEvent.change(width, { target: { value: "20" } });
  fireEvent.blur(width);
  await waitFor(() =>
    expect(setDeviceInvoke).toHaveBeenLastCalledWith(
      tab.id,
      expect.objectContaining({ width: 50, height: 862 }),
    ),
  );
  fireEvent.change(width, { target: { value: "400" } });
  fireEvent.keyDown(width, { key: "Enter" });
  await waitFor(() =>
    expect(setDeviceInvoke).toHaveBeenLastCalledWith(
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
    expect(setDeviceInvoke).toHaveBeenLastCalledWith(
      tab.id,
      expect.objectContaining({ width: 822, height: 862 }),
    ),
  );
  const bottom = document.querySelector(".browser-device-handle--bottom")!;
  fireEvent.pointerDown(bottom, { pointerId: 1, clientX: 0, clientY: 100 });
  fireEvent.pointerMove(bottom, { pointerId: 1, clientX: 0, clientY: 70 });
  await waitFor(() =>
    expect(setDeviceInvoke).toHaveBeenLastCalledWith(
      tab.id,
      expect.objectContaining({ width: 822, height: 832 }),
    ),
  );
});

it("sends only the latest change while an update is in flight", async () => {
  let release = () => {};
  setDeviceInvoke.mockImplementationOnce(
    () =>
      new Promise<undefined>((resolve) => (release = () => resolve(undefined))),
  );
  await openDevice();
  await waitFor(() => expect(setDeviceInvoke).toHaveBeenCalledTimes(1));
  const width = screen.getByRole("spinbutton", { name: "Width" });
  for (const value of ["300", "310", "320"]) {
    fireEvent.change(width, { target: { value } });
    fireEvent.blur(width);
  }
  expect(setDeviceInvoke).toHaveBeenCalledTimes(1);
  await act(async () => release());
  await waitFor(() => expect(setDeviceInvoke).toHaveBeenCalledTimes(2));
  expect(setDeviceInvoke).toHaveBeenLastCalledWith(
    tab.id,
    expect.objectContaining({ width: 320 }),
  );
});

it("closes the toolbar and resets the tab", async () => {
  await openDevice();
  fireEvent.click(screen.getByRole("button", { name: "Close device toolbar" }));
  await waitFor(() =>
    expect(setDeviceInvoke).toHaveBeenLastCalledWith(tab.id, null),
  );
  expect(screen.queryByRole("toolbar", { name: "Device toolbar" })).toBeNull();
  expect(document.querySelector(".browser-surface__frame")).not.toHaveAttribute(
    "data-device",
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

it("reports a failed preset and closes the toolbar Rust confirmed reset", async () => {
  await openDevice();
  await act(async () => undefined);
  setDeviceInvoke.mockRejectedValueOnce({
    message: "device toolbar could not update",
    held: { state: "known", device: null },
  });
  fireEvent.change(screen.getByRole("combobox", { name: "Dimensions:" }), {
    target: { value: "pixel-7" },
  });
  expect(await screen.findByRole("alert")).toHaveTextContent(
    "device toolbar could not update",
  );
  await waitFor(() =>
    expect(
      screen.queryByRole("toolbar", { name: "Device toolbar" }),
    ).toBeNull(),
  );
  expect(document.querySelector(".browser-surface__frame")).not.toHaveAttribute(
    "data-device",
  );
  fireEvent.click(screen.getByRole("button", { name: "More options" }));
  expect(
    screen.getByRole("menuitem", { name: "Show device toolbar" }),
  ).toBeEnabled();
});

it("reopens a failed close on the device Rust confirmed is still held", async () => {
  await openDevice();
  await act(async () => undefined);
  setDeviceInvoke.mockRejectedValueOnce({
    message: "device reset was refused",
    held: {
      state: "known",
      device: { preset: "pixel-7", width: 412, height: 915, scalePercent: 94 },
    },
  });
  fireEvent.click(screen.getByRole("button", { name: "Close device toolbar" }));
  expect(await screen.findByRole("alert")).toHaveTextContent(
    "device reset was refused",
  );
  expect(
    await screen.findByRole("toolbar", { name: "Device toolbar" }),
  ).toBeVisible();
  expect(screen.getByRole("combobox", { name: "Dimensions:" })).toHaveValue(
    "pixel-7",
  );
  expect(screen.getByRole("spinbutton", { name: "Width" })).toHaveValue(412);
  expect(screen.getByRole("spinbutton", { name: "Height" })).toHaveValue(915);
  expect(screen.queryByText("Unconfirmed")).not.toBeInTheDocument();
});

it("marks an unconfirmed reset instead of guessing which device is on", async () => {
  await openDevice();
  await waitFor(() => expect(setDeviceInvoke).toHaveBeenCalledTimes(1));
  setDeviceInvoke.mockRejectedValueOnce({
    message: "device toolbar could not update the page",
    held: { state: "unconfirmed" },
  });
  fireEvent.change(screen.getByRole("combobox", { name: "Dimensions:" }), {
    target: { value: "pixel-7" },
  });
  expect(await screen.findByRole("alert")).toHaveTextContent(
    "device toolbar could not update the page. The page may still be partly emulated",
  );
  expect(screen.getByRole("status")).toHaveTextContent("Unconfirmed");
  expect(screen.getByRole("toolbar", { name: "Device toolbar" })).toBeVisible();
  expect(setDeviceInvoke).toHaveBeenCalledTimes(2);
  // The next change is sent whatever the toolbar shows, and clears the mark.
  fireEvent.click(screen.getByRole("button", { name: "Rotate" }));
  await waitFor(() => expect(setDeviceInvoke).toHaveBeenCalledTimes(3));
  expect(setDeviceInvoke).toHaveBeenLastCalledWith(
    tab.id,
    expect.objectContaining({ preset: "pixel-7", width: 915, height: 412 }),
  );
  await waitFor(() =>
    expect(screen.queryByText("Unconfirmed")).not.toBeInTheDocument(),
  );
});

it("still sends a closed tab's reset when its pending apply fails unconfirmed", async () => {
  const other: WorkbenchTab = { ...tab, id: "browser-2" };
  browserStatus.mockImplementation(async (tabId: string) => ({
    tabId,
    url: "https://example.com/",
    revision: "1",
    loading: false,
    canGoBack: false,
    canGoForward: false,
    zoomPercent: 100,
    device: null,
  }));
  let fail = () => {};
  setDeviceInvoke.mockImplementationOnce(
    () =>
      new Promise<undefined>(
        (_, reject) =>
          (fail = () =>
            reject({
              message: "device toolbar could not update the page",
              held: { state: "unconfirmed" },
            })),
      ),
  );
  const { rerender } = render(<BrowserSurface tab={tab} />);
  await screen.findByLabelText("Secure connection");
  fireEvent.click(screen.getByRole("button", { name: "More options" }));
  fireEvent.click(
    screen.getByRole("menuitem", { name: "Show device toolbar" }),
  );
  await waitFor(() => expect(setDeviceInvoke).toHaveBeenCalledTimes(1));
  fireEvent.click(
    await screen.findByRole("button", { name: "Close device toolbar" }),
  );
  rerender(<BrowserSurface tab={other} />);
  await act(async () => fail());
  await waitFor(() => expect(setDeviceInvoke).toHaveBeenCalledTimes(2));
  expect(setDeviceInvoke).toHaveBeenLastCalledWith(tab.id, null);
});

it("never reapplies the device when unrelated surface state changes", async () => {
  await openDevice();
  await waitFor(() => expect(setDeviceInvoke).toHaveBeenCalledTimes(1));
  const address = screen.getByLabelText("Address or search");
  for (const value of ["e", "ex", "exa"])
    fireEvent.change(address, { target: { value } });
  fireEvent.click(screen.getByRole("button", { name: "More options" }));
  fireEvent.keyDown(document.activeElement ?? document.body, {
    key: "Escape",
  });
  await act(async () => undefined);
  expect(setDeviceInvoke).toHaveBeenCalledTimes(1);
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

it("sends a tab's reset even when the pane switches to an emulated tab", async () => {
  const other: WorkbenchTab = { ...tab, id: "browser-2" };
  const pixel = {
    preset: "pixel-7",
    width: 412,
    height: 915,
    scalePercent: 94,
  };
  browserStatus.mockImplementation(async (tabId: string) => ({
    tabId,
    url: "https://example.com/",
    revision: "1",
    loading: false,
    canGoBack: false,
    canGoForward: false,
    zoomPercent: 100,
    device: tabId === other.id ? pixel : null,
  }));
  let release = () => {};
  setDeviceInvoke.mockImplementationOnce(
    () =>
      new Promise<undefined>((resolve) => (release = () => resolve(undefined))),
  );
  const { rerender } = render(<BrowserSurface tab={tab} />);
  await screen.findByLabelText("Secure connection");
  fireEvent.click(screen.getByRole("button", { name: "More options" }));
  fireEvent.click(
    screen.getByRole("menuitem", { name: "Show device toolbar" }),
  );
  await waitFor(() => expect(setDeviceInvoke).toHaveBeenCalledTimes(1));
  fireEvent.click(
    await screen.findByRole("button", { name: "Close device toolbar" }),
  );
  rerender(<BrowserSurface tab={other} />);
  await screen.findByRole("toolbar", { name: "Device toolbar" });
  await act(async () => release());
  await waitFor(() => expect(setDeviceInvoke).toHaveBeenCalledTimes(3));
  expect(setDeviceInvoke).toHaveBeenNthCalledWith(2, tab.id, null);
  expect(setDeviceInvoke).toHaveBeenNthCalledWith(
    3,
    other.id,
    expect.objectContaining({ preset: "pixel-7", width: 412, height: 915 }),
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
  expect(setDeviceInvoke).not.toHaveBeenCalledWith(other.id, expect.anything());
  expect(document.querySelector("[data-device]")).toBeNull();
});

it("restores the Unconfirmed warning on remount without touching the page", async () => {
  browserStatus.mockResolvedValue({
    tabId: tab.id,
    url: "https://example.com/",
    revision: "1",
    loading: false,
    canGoBack: false,
    canGoForward: false,
    zoomPercent: 100,
    device: null,
    deviceUnconfirmed: true,
  });
  render(<BrowserSurface tab={tab} />);
  await screen.findByRole("toolbar", { name: "Device toolbar" });
  expect(screen.getByRole("status")).toHaveTextContent("Unconfirmed");
  await act(async () => undefined);
  expect(setDeviceInvoke).not.toHaveBeenCalled();
  // Closing is the recovery: it sends the reset whatever the toolbar shows.
  fireEvent.click(screen.getByRole("button", { name: "Close device toolbar" }));
  await waitFor(() =>
    expect(setDeviceInvoke).toHaveBeenCalledWith(tab.id, null),
  );
  expect(setDeviceInvoke).toHaveBeenCalledTimes(1);
});

it("sends the first change after an Unconfirmed restore", async () => {
  browserStatus.mockResolvedValue({
    tabId: tab.id,
    url: "https://example.com/",
    revision: "1",
    loading: false,
    canGoBack: false,
    canGoForward: false,
    zoomPercent: 100,
    device: null,
    deviceUnconfirmed: true,
  });
  render(<BrowserSurface tab={tab} />);
  await screen.findByRole("toolbar", { name: "Device toolbar" });
  fireEvent.change(screen.getByRole("combobox", { name: "Dimensions:" }), {
    target: { value: "iphone-se" },
  });
  await waitFor(() =>
    expect(setDeviceInvoke).toHaveBeenCalledWith(
      tab.id,
      expect.objectContaining({ preset: "iphone-se" }),
    ),
  );
  await waitFor(() =>
    expect(screen.queryByText("Unconfirmed")).not.toBeInTheDocument(),
  );
});

it("emulates again when reopened after an Unconfirmed tab was reset", async () => {
  browserStatus.mockResolvedValue({
    tabId: tab.id,
    url: "https://example.com/",
    revision: "1",
    loading: false,
    canGoBack: false,
    canGoForward: false,
    zoomPercent: 100,
    device: null,
    deviceUnconfirmed: true,
  });
  render(<BrowserSurface tab={tab} />);
  await screen.findByRole("toolbar", { name: "Device toolbar" });
  fireEvent.click(screen.getByRole("button", { name: "Close device toolbar" }));
  await waitFor(() =>
    expect(setDeviceInvoke).toHaveBeenCalledWith(tab.id, null),
  );
  await act(async () => undefined);
  fireEvent.click(screen.getByRole("button", { name: "More options" }));
  fireEvent.click(
    screen.getByRole("menuitem", { name: "Show device toolbar" }),
  );
  await screen.findByRole("toolbar", { name: "Device toolbar" });
  await waitFor(() =>
    expect(setDeviceInvoke).toHaveBeenLastCalledWith(tab.id, {
      preset: "responsive",
      width: 842,
      height: 862,
      scalePercent: 100,
    }),
  );
  expect(screen.queryByText("Unconfirmed")).not.toBeInTheDocument();
});
