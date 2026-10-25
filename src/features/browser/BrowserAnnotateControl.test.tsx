import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
} from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { createRef } from "react";
import {
  BrowserAnnotateControl,
  type AnnotatableNavigation,
} from "./BrowserAnnotateControl";
import { BrowserNavigationControls } from "./BrowserNavigationControls";

const invokeNative = vi.fn(
  async (_command: string, _args?: Record<string, unknown>) => undefined,
);
let announce:
  | ((event: { payload: { tabId: string; active: boolean } }) => void)
  | undefined;
const unlisten = vi.fn();
vi.mock("../../ports/nativeInvoke", () => ({
  isTauriRuntime: () => true,
  invokeNative: (command: string, args?: Record<string, unknown>) =>
    invokeNative(command, args),
}));
vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(async (name: string, handler: typeof announce) => {
    if (name === "browser:annotate") announce = handler;
    return unlisten;
  }),
}));

const navigation: AnnotatableNavigation = {
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
  invokeNative.mockClear();
  unlisten.mockClear();
});
afterEach(cleanup);

async function renderControl(
  props: Partial<Parameters<typeof BrowserAnnotateControl>[0]> = {},
) {
  render(<BrowserAnnotateControl navigation={navigation} ready {...props} />);
  await act(async () => undefined);
  return screen.getByRole("button", { name: "Annotate" });
}

function rustSays(tabId: string, active: boolean) {
  act(() => announce?.({ payload: { tabId, active } }));
}

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
        onControl={vi.fn()}
        onFind={vi.fn()}
        onPrint={vi.fn()}
        onScreenshot={vi.fn()}
        onZoom={vi.fn()}
        onZoomReset={vi.fn()}
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
});

it("turns Annotate on and shows it pressed only once Rust confirms", async () => {
  const button = await renderControl();
  expect(button).toBeEnabled();
  expect(button).toHaveAttribute("aria-pressed", "false");
  expect(button).toHaveAttribute("title", "Annotate the page");
  fireEvent.click(button);
  expect(invokeNative).toHaveBeenCalledWith("browser_control", {
    tabId: "browser-1",
    action: "annotateOn",
  });
  expect(button).toHaveAttribute("aria-pressed", "false");
  rustSays("browser-1", true);
  expect(button).toHaveAttribute("aria-pressed", "true");
  expect(button).toHaveAttribute("title", "Stop annotating (Esc)");
  fireEvent.click(button);
  expect(invokeNative).toHaveBeenLastCalledWith("browser_control", {
    tabId: "browser-1",
    action: "annotateOff",
  });
});

it("follows exits Rust makes itself and ignores other tabs", async () => {
  const button = await renderControl();
  rustSays("browser-1", true);
  rustSays("browser-2", false);
  expect(button).toHaveAttribute("aria-pressed", "true");
  // Navigation, hide or close: Rust leaves Annotate and announces it.
  rustSays("browser-1", false);
  expect(button).toHaveAttribute("aria-pressed", "false");
});

it("leaves on Escape from Sage's UI unless something else handled it", async () => {
  const button = await renderControl();
  fireEvent.keyDown(window, { key: "Escape" });
  expect(invokeNative).not.toHaveBeenCalled();
  rustSays("browser-1", true);
  const handled = new KeyboardEvent("keydown", {
    key: "Escape",
    cancelable: true,
  });
  handled.preventDefault();
  window.dispatchEvent(handled);
  expect(invokeNative).not.toHaveBeenCalled();
  fireEvent.keyDown(document.body, { key: "Escape" });
  expect(invokeNative).toHaveBeenCalledWith("browser_control", {
    tabId: "browser-1",
    action: "annotateOff",
  });
  expect(button).toHaveAttribute("aria-pressed", "true");
  rustSays("browser-1", false);
  fireEvent.keyDown(window, { key: "Escape" });
  expect(invokeNative).toHaveBeenCalledTimes(1);
});

it("renders disabled with the platform reason where there is no inspect overlay", async () => {
  const reason =
    "Annotate needs the WebView2 inspect overlay and is not available on this platform yet";
  const button = await renderControl({
    navigation: { ...navigation, annotateUnavailableReason: reason },
  });
  expect(button).toBeDisabled();
  expect(button).toHaveAttribute("title", reason);
  fireEvent.click(button);
  expect(invokeNative).not.toHaveBeenCalled();
});

it("is disabled until a page is showing", async () => {
  const button = await renderControl({ ready: false });
  expect(button).toBeDisabled();
  expect(button).toHaveAttribute("title", "Open a page to annotate");
  cleanup();
  const none = await renderControl({ navigation: null });
  expect(none).toBeDisabled();
});

it("stops listening and forgets the state when the surface goes away", async () => {
  const button = await renderControl();
  rustSays("browser-1", true);
  expect(button).toHaveAttribute("aria-pressed", "true");
  cleanup();
  expect(unlisten).toHaveBeenCalledTimes(1);
});
