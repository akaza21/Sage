import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { useState } from "react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import {
  clearAgentChatOwner,
  pinAgentChatOwner,
} from "../../app/agentChatOwner";
import { handOffBrowserElement } from "../../app/browserAnnotations";
import { browserElementChatSink } from "../../app/browserElementChatSink";
import { useWorkbenchStore } from "../../app/store";
import {
  parseBrowserElementContext,
  REPEATED_ELEMENT,
  validateAttachedContexts,
  type AttachedContext,
  type BrowserElementContext,
} from "../../domain/attachedContext";
import type { WorkbenchTab } from "../../domain/workbench";
import { SentAnnotations } from "../chat/AnnotationChip";
import { ComposerAttachments } from "../chat/ComposerAttachments";
import { browserElementLabel } from "./BrowserElementChips";

const element = (
  ordinal: number,
  change: Partial<BrowserElementContext> = {},
): BrowserElementContext => ({
  kind: "browserElement",
  target: "element",
  ordinal,
  tabId: "browser-1",
  url: "https://example.com/cart",
  title: "Cart",
  selector: "main > :nth-child(2 of button)",
  role: "button",
  name: "Place order",
  text: "Place order",
  box: { x: 4, y: 8, width: 160, height: 40 },
  comment: "Disable until the terms are ticked",
  ...change,
});

const chat: WorkbenchTab = {
  id: "chat-a",
  kind: "chat",
  title: "Chat: A",
  position: 0,
  pinned: true,
  dirty: false,
  status: "idle",
};

beforeEach(() => {
  useWorkbenchStore.setState({
    phase: "ready",
    project: { id: "project-a", name: "A", roots: [] },
    tabs: [chat],
    activeTabId: chat.id,
    projectGeneration: 0,
    nativeTransitionEpoch: 0,
    projectTransitioning: false,
    chatUiById: {},
  });
});
afterEach(() => {
  cleanup();
  clearAgentChatOwner();
  vi.useRealTimers();
});

function renderChips(contexts: readonly AttachedContext[]) {
  const onRemove = vi.fn();
  const view = render(
    <ComposerAttachments
      contexts={contexts}
      disabled={false}
      onRemove={onRemove}
    />,
  );
  return { onRemove, ...view };
}

it("shows one element as its own chip with the contract label", async () => {
  renderChips([element(1)]);
  expect(
    await screen.findByRole("button", {
      name: 'Element 1 · button "Place order"',
    }),
  ).toBeInTheDocument();
  expect(
    browserElementLabel(element(2, { name: "n".repeat(80) })),
  ).toHaveLength(48);
});

it("collapses several elements into one N annotations chip beside other context", async () => {
  const page = {
    kind: "browserPage" as const,
    tabId: "browser-1",
    url: "https://example.com/",
    title: "Home",
    text: "",
  };
  renderChips([element(1), page, element(2)]);
  expect(
    await screen.findByRole("button", {
      name: "2 annotations on the page",
    }),
  ).toBeInTheDocument();
  expect(screen.queryByText(/Element 1 ·/)).toBeNull();
  expect(screen.getByText("Page · Home")).toBeInTheDocument();
});

it("opens a card on hover with role, name, comment and page, and closes after leaving", async () => {
  renderChips([element(1), element(2, { name: "", comment: "" })]);
  const chip = await screen.findByRole("button", {
    name: "2 annotations on the page",
  });
  vi.useFakeTimers();
  fireEvent.mouseEnter(chip.parentElement!);
  const card = screen.getByRole("dialog", { name: "Page annotations" });
  expect(chip).toHaveAttribute("aria-expanded", "true");
  const [first, second] = Array.from(card.querySelectorAll("section"));
  expect(within(card).getAllByRole("code")).toHaveLength(2);
  expect(first).toHaveTextContent("button");
  expect(first).toHaveTextContent("Place order");
  expect(first).toHaveTextContent("Disable until the terms are ticked");
  expect(first).toHaveTextContent("Cart");
  expect(second).toHaveTextContent("Element 2");
  expect(second).toHaveTextContent("No comment");
  fireEvent.mouseLeave(chip.parentElement!);
  act(() => vi.advanceTimersByTime(200));
  expect(screen.queryByRole("dialog")).toBeNull();
});

it("removes one element from the card, or all from the chip", async () => {
  const { onRemove } = renderChips([element(1), element(2)]);
  const chip = await screen.findByRole("button", {
    name: "2 annotations on the page",
  });
  fireEvent.focus(chip);
  fireEvent.click(screen.getByRole("button", { name: "Delete element 2" }));
  expect(onRemove).toHaveBeenCalledWith(element(2));
  onRemove.mockClear();
  fireEvent.click(
    screen.getByRole("button", { name: "Remove all page annotations" }),
  );
  expect(onRemove.mock.calls).toEqual([[element(1)], [element(2)]]);
});

it("closes the card on Escape and returns focus to the chip", async () => {
  renderChips([element(1)]);
  const chip = await screen.findByRole("button", {
    name: 'Element 1 · button "Place order"',
  });
  fireEvent.focus(chip);
  const card = screen.getByRole("dialog");
  fireEvent.keyDown(card, { key: "Escape" });
  expect(screen.queryByRole("dialog")).toBeNull();
  expect(chip).toHaveFocus();
});

it("puts a confirmed element in the pinned Agent Chat's draft, numbered from the draft", () => {
  expect(browserElementChatSink.nextOrdinal()).toBeNull();
  expect(handOffBrowserElement(element(1))).toBe(false);
  pinAgentChatOwner(chat.id);
  expect(browserElementChatSink.nextOrdinal()).toBe(1);
  expect(browserElementChatSink.attach(element(1))).toBe(true);
  expect(browserElementChatSink.attach(element(3))).toBe(true);
  expect(browserElementChatSink.nextOrdinal()).toBe(2);
  const draft =
    useWorkbenchStore.getState().chatUiById[chat.id]!.attachedContext!;
  expect(draft).toEqual([element(1), element(3)]);
  expect(validateAttachedContexts(draft)).toEqual(draft);
});

it("refuses a ninth context in the draft", () => {
  pinAgentChatOwner(chat.id);
  for (let ordinal = 1; ordinal <= 8; ordinal += 1)
    expect(browserElementChatSink.attach(element(ordinal))).toBe(true);
  expect(browserElementChatSink.nextOrdinal()).toBeNull();
});

it("refuses an element whose number the draft already holds, keeping both comments", () => {
  pinAgentChatOwner(chat.id);
  expect(browserElementChatSink.attach(element(1))).toBe(true);
  expect(
    browserElementChatSink.attach(element(1, { comment: "Second thought" })),
  ).toBe(false);
  expect(
    browserElementChatSink.attach(element(1, { tabId: "browser-2" })),
  ).toBe(false);
  expect(
    useWorkbenchStore.getState().chatUiById[chat.id]!.attachedContext,
  ).toEqual([element(1)]);
});

it("refuses repeated element numbers at send with their own reason", () => {
  expect(() =>
    validateAttachedContexts([element(1), element(1, { tabId: "browser-2" })]),
  ).toThrow(REPEATED_ELEMENT);
});

it("admits tabs and line breaks in a comment but no other control or bidi character", () => {
  expect(
    parseBrowserElementContext(element(1, { comment: "one\n\ttwo" })),
  ).not.toBeNull();
  for (const comment of ["bell\u0007", "a\rb", "hidden‮comment"])
    expect(parseBrowserElementContext(element(1, { comment }))).toBeNull();
});

it("cuts a long label by characters, never inside a surrogate pair", () => {
  const label = browserElementLabel(
    element(1, { name: `${"n".repeat(26)}😀😀😀` }),
  );
  expect([...label]).toHaveLength(48);
  expect(label).toMatch(/😀…$/u);
  expect(label).not.toMatch(/[\ud800-\udbff](?![\udc00-\udfff])/u);
});

it("shows the URL and selector in the chip and each card entry tooltip", async () => {
  renderChips([element(1)]);
  const chip = await screen.findByRole("button", {
    name: 'Element 1 · button "Place order"',
  });
  const tip = "https://example.com/cart\nmain > :nth-child(2 of button)";
  expect(chip).toHaveAttribute("title", tip);
  fireEvent.focus(chip);
  const entry = screen.getByRole("dialog").querySelector("header")!;
  expect(entry).toHaveAttribute("title", tip);
});

function Draft({ initial }: { initial: readonly AttachedContext[] }) {
  const [contexts, setContexts] = useState(initial);
  return (
    <form>
      <textarea aria-label="Message" />
      <ComposerAttachments
        contexts={contexts}
        disabled={false}
        onRemove={(removed) =>
          setContexts((current) => current.filter((item) => item !== removed))
        }
      />
    </form>
  );
}

it("moves focus into the card container, never onto a delete button, and Tab returns to the chip", async () => {
  render(<Draft initial={[element(1), element(2)]} />);
  const chip = await screen.findByRole("button", {
    name: "2 annotations on the page",
  });
  fireEvent.click(chip);
  const card = screen.getByRole("dialog");
  await waitFor(() => expect(card).toHaveFocus());
  fireEvent.keyDown(card, { key: "Tab", shiftKey: true });
  expect(chip).toHaveFocus();
  expect(screen.queryByRole("dialog")).toBeNull();
  fireEvent.click(chip);
  const last = screen.getByRole("button", { name: "Delete element 2" });
  last.focus();
  fireEvent.keyDown(last, { key: "Tab" });
  expect(
    screen.getByRole("button", { name: "Remove all page annotations" }),
  ).toHaveFocus();
});

it("keeps focus in the card after a delete, and on the composer after the last", async () => {
  render(<Draft initial={[element(1), element(2)]} />);
  fireEvent.focus(
    await screen.findByRole("button", { name: "2 annotations on the page" }),
  );
  fireEvent.click(screen.getByRole("button", { name: "Delete element 1" }));
  expect(
    screen.getByRole("button", { name: "Delete element 2" }),
  ).toHaveFocus();
  fireEvent.click(screen.getByRole("button", { name: "Delete element 2" }));
  expect(screen.getByRole("textbox", { name: "Message" })).toHaveFocus();
  expect(screen.queryByRole("dialog")).toBeNull();
});

it("shows a sent turn's elements and pages read-only above the bubble", async () => {
  const page = {
    kind: "browserPage" as const,
    tabId: "browser-1",
    url: "https://example.com/",
    title: "Home",
    text: "Hello",
  };
  render(<SentAnnotations contexts={[element(1), page, element(2)]} />);
  const list = await screen.findByRole("list", { name: "Page context" });
  const chip = within(list).getByRole("button", {
    name: "2 annotations on the page",
  });
  expect(
    within(list).getByText("Page · Home").closest("[title]"),
  ).toHaveAttribute("title", "https://example.com/");
  expect(within(list).queryByRole("button", { name: /Remove/ })).toBeNull();
  fireEvent.focus(chip);
  expect(within(screen.getByRole("dialog")).queryByRole("button")).toBeNull();
});

it("keeps two sent pages that share a URL apart", async () => {
  const errors = vi.spyOn(console, "error").mockImplementation(() => {});
  const page = (title: string) => ({
    kind: "browserPage" as const,
    tabId: "browser-1",
    url: "https://example.com/",
    title,
    text: title,
  });
  render(<SentAnnotations contexts={[page("Before"), page("After")]} />);
  const list = await screen.findByRole("list", { name: "Page context" });
  expect(within(list).getByText("Page · Before")).toBeInTheDocument();
  expect(within(list).getByText("Page · After")).toBeInTheDocument();
  expect(errors.mock.calls.flat().join(" ")).not.toMatch(/same key/);
  errors.mockRestore();
});
