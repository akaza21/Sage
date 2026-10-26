import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { createRef } from "react";
import type { BrowserNavigation } from "../../ports/nativeBrowserPort";
import { BrowserAnnotateControl } from "./BrowserAnnotateControl";
import { BrowserNavigationControls } from "./BrowserNavigationControls";

const onControl = vi.fn();
let announce:
  | ((event: { payload: { tabId: string; active: boolean } }) => void)
  | undefined;
const unlisten = vi.fn();
vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(async (name: string, handler: typeof announce) => {
    if (name === "browser:annotate") announce = handler;
    return unlisten;
  }),
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
  unlisten.mockClear();
});
afterEach(cleanup);

async function renderControl(current: BrowserNavigation | null = navigation) {
  render(<BrowserAnnotateControl navigation={current} onControl={onControl} />);
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
        onControl={onControl}
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
  // Navigation, hide, close or Escape in the page: Rust leaves and says so.
  rustSays("browser-1", false);
  expect(button).toHaveAttribute("aria-pressed", "false");
});

it("leaves on Escape at the control only while annotating", async () => {
  const button = await renderControl();
  fireEvent.keyDown(button, { key: "Escape" });
  expect(onControl).not.toHaveBeenCalled();
  rustSays("browser-1", true);
  fireEvent.keyDown(button, { key: "Enter" });
  expect(onControl).not.toHaveBeenCalled();
  fireEvent.keyDown(button, { key: "Escape" });
  expect(onControl).toHaveBeenCalledWith("annotateOff");
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
