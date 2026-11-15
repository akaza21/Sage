import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import type * as NativeInvoke from "../../ports/nativeInvoke";
import { createRef } from "react";
import type {
  BrowserAnnotateChange,
  BrowserAnnotateSelection,
  BrowserNavigation,
} from "../../ports/nativeBrowserPort";
import { BrowserAnnotateControl } from "./BrowserAnnotateControl";
import { BrowserNavigationControls } from "./BrowserNavigationControls";

const onControl = vi.fn();
const onError = vi.fn();
const invokeNative = vi.fn();
let announce: ((event: { payload: BrowserAnnotateChange }) => void) | undefined;
const unlisten = vi.fn();
vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(async (name: string, handler: typeof announce) => {
    if (name === "browser:annotate") announce = handler;
    return unlisten;
  }),
}));
vi.mock("../../ports/nativeInvoke", async (importOriginal) => ({
  ...(await importOriginal<typeof NativeInvoke>()),
  invokeNative: (...args: unknown[]) => invokeNative(...args),
}));

const navigation: BrowserNavigation = {
  tabId: "browser-1",
  url: "https://example.com/",
  revision: "3",
  loading: false,
  canGoBack: false,
  canGoForward: false,
  zoomPercent: 100,
};

beforeEach(() => {
  announce = undefined;
  onControl.mockClear();
  onError.mockClear();
  unlisten.mockClear();
  invokeNative.mockReset();
  invokeNative.mockResolvedValue({
    jpegBase64: "/9j/AAAA",
    width: 1265,
    height: 720,
  });
});
afterEach(cleanup);

async function renderControl(current: BrowserNavigation | null = navigation) {
  render(
    <BrowserAnnotateControl
      navigation={current}
      onControl={onControl}
      onError={onError}
    />,
  );
  await act(async () => undefined);
  return screen.getByRole("button", { name: "Annotate" });
}

function rustSays(
  tabId: string,
  active: boolean,
  more: Partial<BrowserAnnotateChange> = {},
) {
  act(() => announce?.({ payload: { tabId, active, ...more } }));
}

const selection: BrowserAnnotateSelection = {
  label: "Press me",
  box: { x: 8, y: 140, width: 80, height: 24 },
  viewport: {
    x: 0,
    y: 100,
    width: 1280,
    height: 720,
    clientWidth: 1265,
    clientHeight: 720,
    scale: 1,
  },
};

it("sits between the navigation group and the address", () => {
  render(
    <div className="browser-surface__toolbar">
      <BrowserNavigationControls
        navigation={navigation}
        page={{
          origin: "https://example.com",
          url: "https://example.com/",
          transport: "secure",
        }}
        address="https://example.com/"
        addressField={createRef()}
        onAddressChange={vi.fn()}
        onNavigate={vi.fn()}
        onControl={onControl}
        onError={onError}
        onFind={vi.fn()}
        onPrint={vi.fn()}
        onScreenshot={vi.fn()}
        onZoom={vi.fn()}
        onZoomReset={vi.fn()}
        deviceOpen={false}
        onDevice={vi.fn()}
        onDownloads={vi.fn()}
        onHistory={vi.fn()}
        onSettings={vi.fn()}
        onCopy={vi.fn()}
        onExternal={vi.fn()}
        clearDataControl={null}
      />
    </div>,
  );
  const reload = screen.getByRole("button", { name: "Reload" });
  const annotate = screen.getByRole("button", { name: "Annotate" });
  const address = screen.getByRole("textbox", { name: "Address or search" });
  expect(
    reload.compareDocumentPosition(annotate) & Node.DOCUMENT_POSITION_FOLLOWING,
  ).toBeTruthy();
  expect(
    annotate.compareDocumentPosition(address) &
      Node.DOCUMENT_POSITION_FOLLOWING,
  ).toBeTruthy();
  fireEvent.click(annotate);
  expect(onControl).toHaveBeenCalledWith("annotateOn");
});

it("asks Rust to turn Annotate on and shows it pressed only once Rust confirms", async () => {
  const button = await renderControl();
  expect(button).toBeEnabled();
  expect(button).toHaveAttribute("aria-pressed", "false");
  fireEvent.click(button);
  expect(onControl).toHaveBeenCalledWith("annotateOn");
  expect(button).toHaveAttribute("aria-pressed", "false");
  rustSays("browser-1", true);
  expect(button).toHaveAttribute("aria-pressed", "true");
  fireEvent.click(button);
  expect(onControl).toHaveBeenLastCalledWith("annotateOff");
});

it("follows exits Rust makes itself and ignores other tabs", async () => {
  const button = await renderControl();
  rustSays("browser-1", true);
  rustSays("browser-2", false);
  expect(button).toHaveAttribute("aria-pressed", "true");
  rustSays("browser-1", false);
  expect(button).toHaveAttribute("aria-pressed", "false");
});

it("on Escape at the control drops a waiting element first, then leaves", async () => {
  const button = await renderControl();
  fireEvent.keyDown(button, { key: "Escape" });
  expect(onControl).not.toHaveBeenCalled();
  rustSays("browser-1", true, { selection });
  fireEvent.keyDown(button, { key: "Enter" });
  expect(onControl).not.toHaveBeenCalled();
  fireEvent.keyDown(button, { key: "Escape" });
  expect(onControl).toHaveBeenLastCalledWith("annotateCancel");
  rustSays("browser-1", true);
  fireEvent.keyDown(button, { key: "Escape" });
  expect(onControl).toHaveBeenLastCalledWith("annotateOff");
});

it("consumes active annotation Escape before an enclosing resource pane", async () => {
  const closePane = vi.fn();
  render(
    <div onKeyDown={closePane}>
      <BrowserAnnotateControl
        navigation={navigation}
        onControl={onControl}
        onError={onError}
      />
    </div>,
  );
  await act(async () => undefined);
  const button = screen.getByRole("button", { name: "Annotate" });
  rustSays("browser-1", true, { selection });
  expect(fireEvent.keyDown(button, { key: "Escape" })).toBe(false);
  expect(onControl).toHaveBeenLastCalledWith("annotateCancel");
  expect(closePane).not.toHaveBeenCalled();
  rustSays("browser-1", true);
  expect(fireEvent.keyDown(button, { key: "Escape" })).toBe(false);
  expect(onControl).toHaveBeenLastCalledWith("annotateOff");
  expect(closePane).not.toHaveBeenCalled();
  rustSays("browser-1", false);
  fireEvent.keyDown(button, { key: "Escape" });
  expect(closePane).toHaveBeenCalledOnce();
});

it("renders disabled with the reason Rust gives", async () => {
  const reason =
    "Annotate needs the WebView2 inspect overlay and is not available on this platform yet";
  const button = await renderControl({
    ...navigation,
    annotateUnavailableReason: reason,
  });
  expect(button).toBeDisabled();
  expect(button).toHaveAttribute("title", reason);
  fireEvent.click(button);
  expect(onControl).not.toHaveBeenCalled();
});

it("is disabled before the tab reports any state", async () => {
  const button = await renderControl(null);
  expect(button).toBeDisabled();
  expect(announce).toBeUndefined();
});

it("stops listening and forgets the state when the surface goes away", async () => {
  const button = await renderControl();
  rustSays("browser-1", true);
  expect(button).toHaveAttribute("aria-pressed", "true");
  cleanup();
  await waitFor(() => expect(unlisten).toHaveBeenCalledTimes(1));
});

it("covers the page frame with the comment box while an element waits", async () => {
  render(
    <section className="browser-surface">
      <BrowserAnnotateControl
        navigation={navigation}
        onControl={onControl}
        onError={onError}
      />
      <div className="browser-surface__viewport" data-testid="frame" />
    </section>,
  );
  await act(async () => undefined);
  rustSays("browser-1", true);
  expect(screen.queryByRole("dialog")).toBeNull();
  rustSays("browser-1", true, { selection });
  const dialog = await screen.findByRole("dialog", {
    name: "Comment on Press me",
  });
  expect(screen.getByTestId("frame")).toContainElement(dialog);
  await waitFor(() =>
    expect(invokeNative).toHaveBeenCalledWith("browser_control", {
      tabId: "browser-1",
      action: "annotateFrame",
    }),
  );
  rustSays("browser-1", true);
  expect(screen.queryByRole("dialog")).toBeNull();
  rustSays("browser-1", true, { selection });
  await screen.findByRole("dialog");
  rustSays("browser-1", false);
  expect(screen.queryByRole("dialog")).toBeNull();
});

it("reports a failed pick through the surface's error banner", async () => {
  await renderControl();
  rustSays("browser-1", true, { error: "browser element unavailable" });
  expect(onError).toHaveBeenCalledWith("browser element unavailable");
  expect(screen.queryByRole("dialog")).toBeNull();
});

it("drops an element left waiting by an earlier chrome once it listens", async () => {
  await renderControl();
  expect(announce).toBeDefined();
  expect(invokeNative).toHaveBeenCalledWith("browser_control", {
    tabId: "browser-1",
    action: "annotateCancel",
  });
});
