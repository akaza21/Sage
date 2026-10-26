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
import {
  registerBrowserElementSink,
  type BrowserElementContext,
} from "../../app/browserAnnotations";
import type { BrowserAnnotateSelection } from "../../ports/nativeBrowserPort";
import BrowserAnnotateComment, {
  BrowserAnnotatePopover,
} from "./BrowserAnnotatePopover";

const invokeNative = vi.fn();
vi.mock("../../ports/nativeInvoke", async (importOriginal) => ({
  ...(await importOriginal<typeof NativeInvoke>()),
  invokeNative: (...args: unknown[]) => invokeNative(...args),
}));

const selection: BrowserAnnotateSelection = {
  role: "button",
  name: "Press me",
  box: { x: 128, y: 172, width: 256, height: 36 },
  viewport: {
    x: 0,
    y: 100,
    width: 1280,
    height: 720,
    clientWidth: 1265,
    clientHeight: 720,
    scale: 1.5,
  },
};
const frame = { jpegBase64: "/9j/AAAA", width: 1898, height: 1080 };
const item: BrowserElementContext = {
  kind: "browserElement",
  target: "element",
  ordinal: 2,
  tabId: "browser-1",
  url: "https://example.com/",
  title: "Example",
  selector: "#press",
  role: "button",
  name: "Press me",
  text: "Press me",
  box: { x: 128, y: 172, width: 256, height: 36 },
  comment: "Make it blue",
};
const onConfirm = vi.fn();
const onCancel = vi.fn();
let unregister = () => {};

beforeEach(() => {
  vi.clearAllMocks();
  invokeNative.mockReset();
});
afterEach(() => {
  cleanup();
  unregister();
});

function popover(props: Partial<Parameters<typeof BrowserAnnotatePopover>[0]>) {
  return render(
    <BrowserAnnotatePopover
      selection={selection}
      frame={frame}
      ordinal={3}
      blocked={null}
      error={null}
      onConfirm={onConfirm}
      onCancel={onCancel}
      {...props}
    />,
  );
}

it("draws the frozen frame, the outline and the numbered badge over the element", () => {
  const { container } = popover({});
  const image = container.querySelector<HTMLImageElement>(
    ".browser-annotate__frame",
  )!;
  expect(image.src).toBe("data:image/jpeg;base64,/9j/AAAA");
  // The frame covers the viewport without its scrollbar.
  expect(image.style.width).toBe(`${(1265 / 1280) * 100}%`);
  const outline = container.querySelector<HTMLElement>(
    ".browser-annotate__outline",
  )!;
  expect(outline.style.left).toBe("10%");
  expect(outline.style.top).toBe("10%");
  expect(outline.style.width).toBe("20%");
  expect(outline.style.height).toBe("5%");
  const badge = container.querySelector<HTMLElement>(
    ".browser-annotate__badge",
  )!;
  expect(badge).toHaveTextContent("3");
  expect([badge.style.left, badge.style.top]).toEqual(["10%", "10%"]);
});

it("puts the comment box under the element, or above it near the bottom", () => {
  const { container, unmount } = popover({});
  const pill = () =>
    container.querySelector<HTMLElement>(".browser-annotate__pill")!;
  expect(pill().style.top).toBe("calc(15% + 8px)");
  // jsdom drops clamp(); the UI rules measure the horizontal placement.
  unmount();
  const low = popover({
    selection: { ...selection, box: { ...selection.box, y: 676 } },
  });
  const lowPill = low.container.querySelector<HTMLElement>(
    ".browser-annotate__pill",
  )!;
  expect(lowPill.style.top).toBe("");
  expect(lowPill.style.bottom).toBe("calc(20% + 8px)");
});

it("confirms the typed comment on Enter or the tick", () => {
  popover({});
  const input = screen.getByRole("textbox", { name: "Comment" });
  expect(input).toHaveAttribute("placeholder", "Add a comment…");
  expect(input).toHaveFocus();
  expect(screen.getByRole("button", { name: "Adjust" })).toBeDisabled();
  fireEvent.change(input, { target: { value: "Make it blue" } });
  fireEvent.submit(input.closest("form")!);
  expect(onConfirm).toHaveBeenLastCalledWith("Make it blue");
  fireEvent.click(screen.getByRole("button", { name: "Add comment" }));
  expect(onConfirm).toHaveBeenCalledTimes(2);
});

it("cancels on Escape or a click on the frozen page", () => {
  const { container } = popover({});
  fireEvent.keyDown(screen.getByRole("textbox", { name: "Comment" }), {
    key: "Escape",
  });
  expect(onCancel).toHaveBeenCalledTimes(1);
  fireEvent.click(container.querySelector(".browser-annotate__frame")!);
  expect(onCancel).toHaveBeenCalledTimes(2);
});

it("will not confirm when blocked or over 4 KB, and says why", () => {
  const { unmount } = popover({ blocked: "No composer" });
  const tick = screen.getByRole("button", { name: "Add comment" });
  expect(tick).toBeDisabled();
  expect(tick).toHaveAttribute("title", "No composer");
  fireEvent.submit(tick.closest("form")!);
  expect(onConfirm).not.toHaveBeenCalled();
  unmount();
  popover({});
  fireEvent.change(screen.getByRole("textbox", { name: "Comment" }), {
    target: { value: "é".repeat(2049) },
  });
  expect(screen.getByRole("button", { name: "Add comment" })).toBeDisabled();
});

it("says the composer cannot take elements until Chat registers a sink", async () => {
  invokeNative.mockResolvedValue(frame);
  render(<BrowserAnnotateComment tabId="browser-1" selection={selection} />);
  const tick = screen.getByRole("button", { name: "Add comment" });
  expect(tick).toBeDisabled();
  expect(tick).toHaveAttribute(
    "title",
    "The composer cannot take page elements yet",
  );
  await act(async () => undefined);
});

it("numbers the comment from the sink, confirms it in Rust, hands the item off, then ends the wait", async () => {
  const attach = vi.fn();
  unregister = registerBrowserElementSink({ nextOrdinal: () => 2, attach });
  invokeNative.mockImplementation(
    async (_: string, args: { action: string }) =>
      args.action === "annotateFrame" ? frame : item,
  );
  const { container } = render(
    <BrowserAnnotateComment tabId="browser-1" selection={selection} />,
  );
  await waitFor(() =>
    expect(container.querySelector(".browser-annotate__frame")).not.toBeNull(),
  );
  expect(container.querySelector(".browser-annotate__badge")).toHaveTextContent(
    "2",
  );
  fireEvent.change(screen.getByRole("textbox", { name: "Comment" }), {
    target: { value: "Make it blue" },
  });
  fireEvent.click(screen.getByRole("button", { name: "Add comment" }));
  await waitFor(() => expect(attach).toHaveBeenCalledWith(item));
  expect(invokeNative).toHaveBeenCalledWith("browser_control", {
    tabId: "browser-1",
    action: "annotateConfirm",
    ordinal: 2,
    comment: "Make it blue",
  });
  await waitFor(() =>
    expect(invokeNative).toHaveBeenLastCalledWith("browser_control", {
      tabId: "browser-1",
      action: "annotateCancel",
    }),
  );
  expect(screen.queryByRole("alert")).toBeNull();
});

it("keeps an item the composer refuses waiting, says why, and cancels through Rust", async () => {
  const attach = vi.fn();
  unregister = registerBrowserElementSink({ nextOrdinal: () => 1, attach });
  invokeNative.mockImplementation(
    async (_: string, args: { action: string }) =>
      args.action === "annotateFrame"
        ? frame
        : args.action === "annotateConfirm"
          ? { ...item, url: "https://user:pw@example.com/" }
          : null,
  );
  render(<BrowserAnnotateComment tabId="browser-1" selection={selection} />);
  fireEvent.click(screen.getByRole("button", { name: "Add comment" }));
  expect(await screen.findByRole("alert")).toHaveTextContent(
    "The page element could not be attached",
  );
  expect(attach).not.toHaveBeenCalled();
  // The element still waits: nothing released it in Rust.
  expect(invokeNative).not.toHaveBeenCalledWith("browser_control", {
    tabId: "browser-1",
    action: "annotateCancel",
  });
  expect(screen.getByRole("dialog")).toBeInTheDocument();
  fireEvent.keyDown(screen.getByRole("dialog"), { key: "Escape" });
  await waitFor(() =>
    expect(invokeNative).toHaveBeenCalledWith("browser_control", {
      tabId: "browser-1",
      action: "annotateCancel",
    }),
  );
});

it("shows a refused confirmation in the box and keeps the element waiting", async () => {
  unregister = registerBrowserElementSink({
    nextOrdinal: () => 1,
    attach: vi.fn(),
  });
  invokeNative.mockImplementation(
    async (_: string, args: { action: string }) => {
      if (args.action === "annotateFrame") return frame;
      throw "browser element text rejected";
    },
  );
  render(<BrowserAnnotateComment tabId="browser-1" selection={selection} />);
  fireEvent.click(screen.getByRole("button", { name: "Add comment" }));
  expect(await screen.findByRole("alert")).toHaveTextContent(
    "browser element text rejected",
  );
  expect(screen.getByRole("dialog")).toBeInTheDocument();
});
