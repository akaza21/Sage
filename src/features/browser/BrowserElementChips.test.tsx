import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
  within,
} from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import {
  clearAgentChatOwner,
  pinAgentChatOwner,
} from "../../app/agentChatOwner";
import { handOffBrowserElement } from "../../app/browserAnnotations";
import { browserElementChatSink } from "../../app/browserElementChatSink";
import { useWorkbenchStore } from "../../app/store";
import {
  validateAttachedContexts,
  type AttachedContext,
  type BrowserElementContext,
} from "../../domain/attachedContext";
import type { WorkbenchTab } from "../../domain/workbench";
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
    await screen.findByRole("button", { name: "2 annotations" }),
  ).toBeInTheDocument();
  expect(screen.queryByText(/Element 1 ·/)).toBeNull();
  expect(screen.getByText("Page · Home")).toBeInTheDocument();
});

it("opens a card on hover with role, name, comment and page, and closes after leaving", async () => {
  renderChips([element(1), element(2, { name: "", comment: "" })]);
  const chip = await screen.findByRole("button", { name: "2 annotations" });
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
  const chip = await screen.findByRole("button", { name: "2 annotations" });
  fireEvent.focus(chip);
  fireEvent.click(screen.getByRole("button", { name: "Delete element 2" }));
  expect(onRemove).toHaveBeenCalledWith(element(2));
  onRemove.mockClear();
  fireEvent.click(
    screen.getByRole("button", { name: "Remove all annotations" }),
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
  // The draft is what the send path validates and passes on as
  // `attachedContext` (acpActionController.test.ts covers the request).
  expect(validateAttachedContexts(draft)).toEqual(draft);
});

it("refuses a ninth context in the draft", () => {
  pinAgentChatOwner(chat.id);
  for (let ordinal = 1; ordinal <= 8; ordinal += 1)
    expect(browserElementChatSink.attach(element(ordinal))).toBe(true);
  expect(browserElementChatSink.nextOrdinal()).toBeNull();
});
