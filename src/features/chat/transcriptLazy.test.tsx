import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { emptyChatOwnershipEntry } from "../../app/chatOwnershipProjection";
import { useWorkbenchStore } from "../../app/store";

vi.mock("./TranscriptInteractions", () => ({
  TranscriptInteractions: (props: { initialKeyboard: boolean }) => (
    <p data-testid="interactions">
      {props.initialKeyboard ? "keyboard" : "pointer"}
    </p>
  ),
  SideChatHome: () => <p data-testid="side-home">Side chat</p>,
}));

import { TranscriptInteractionsGate, TranscriptPart } from "./transcriptLazy";

const initialState = useWorkbenchStore.getState();
const quote = {
  kind: "transcriptQuote" as const,
  ordinal: 1,
  sourceTurnId: "t",
  sourceItemId: "i",
  text: "quoted",
  startOffset: 0,
  endOffset: 6,
};
let content: HTMLElement;
let contentRef: { current: HTMLElement | null };

beforeEach(() => {
  content = document.createElement("div");
  content.innerHTML =
    '<article data-selectable-answer="t" data-item-id="i" tabindex="0"><p>Answer text</p></article><p id="outside" tabindex="0">Other</p>';
  document.body.append(content);
  contentRef = { current: content };
});

afterEach(() => {
  cleanup();
  window.getSelection()?.removeAllRanges();
  content.remove();
  useWorkbenchStore.setState(initialState, true);
});

function selectIn(selector: string) {
  const range = document.createRange();
  range.selectNodeContents(content.querySelector(selector)!);
  window.getSelection()!.removeAllRanges();
  window.getSelection()!.addRange(range);
}

it("loads nothing for clicks without a selection, then arms once on a live selection", async () => {
  const add = vi.spyOn(content, "addEventListener");
  const remove = vi.spyOn(content, "removeEventListener");
  render(
    <TranscriptInteractionsGate chatId="c" focused contentRef={contentRef} />,
  );
  fireEvent.pointerUp(content);
  fireEvent.keyUp(content, { key: "a" });
  expect(screen.queryByTestId("interactions")).toBeNull();
  selectIn("article p");
  fireEvent.pointerUp(content);
  expect(await screen.findByTestId("interactions")).toHaveTextContent(
    "pointer",
  );
  // The core trigger detaches as soon as the lazy chunk owns the listeners.
  expect(remove.mock.calls.length).toBe(add.mock.calls.length);
});

it("arms from the menu key only on a focused answer, for keyboard users", async () => {
  render(
    <TranscriptInteractionsGate chatId="c" focused contentRef={contentRef} />,
  );
  const outside = content.querySelector<HTMLElement>("#outside")!;
  fireEvent.keyDown(outside, { key: "F10", shiftKey: true });
  const answer = content.querySelector<HTMLElement>("article")!;
  fireEvent.keyDown(answer, { key: "a" });
  expect(screen.queryByTestId("interactions")).toBeNull();
  fireEvent.keyDown(answer, { key: "ContextMenu" });
  expect(await screen.findByTestId("interactions")).toHaveTextContent(
    "keyboard",
  );
});

it("mounts for pending annotations so their badges survive a remount", async () => {
  useWorkbenchStore.setState({
    chatUiById: {
      c: { ...emptyChatOwnershipEntry(), attachedContext: [quote] },
    },
  });
  render(
    <TranscriptInteractionsGate chatId="c" focused contentRef={contentRef} />,
  );
  expect(await screen.findByTestId("interactions")).toBeVisible();
});

it("renders nothing, rather than throwing, for an entry without attached context", () => {
  // Untyped fixtures (tests/ui is outside tsconfig) have seeded entries with
  // no attachedContext; the gate must not tear down the Chat pane for them.
  const { attachedContext: _omitted, ...partial } = emptyChatOwnershipEntry();
  useWorkbenchStore.setState({
    chatUiById: { c: partial as ReturnType<typeof emptyChatOwnershipEntry> },
  });
  expect(() =>
    render(
      <TranscriptInteractionsGate chatId="c" focused contentRef={contentRef} />,
    ),
  ).not.toThrow();
  expect(screen.queryByTestId("interactions")).toBeNull();
});

it("renders a named part of the lazy chunk on demand", async () => {
  render(<TranscriptPart part="SideChatHome" />);
  expect(await screen.findByTestId("side-home")).toHaveTextContent("Side chat");
});

it("arms pointer mode, not keyboard mode, when a mouse selection meets Ctrl+C", async () => {
  render(
    <TranscriptInteractionsGate chatId="c" focused contentRef={contentRef} />,
  );
  selectIn("article p");
  fireEvent.keyUp(content, { key: "c", ctrlKey: true });
  expect(screen.queryByTestId("interactions")).toBeNull();
  fireEvent.pointerUp(content);
  expect(await screen.findByTestId("interactions")).toHaveTextContent(
    "pointer",
  );
});

it("arms keyboard mode from a Shift+Arrow selection", async () => {
  render(
    <TranscriptInteractionsGate chatId="c" focused contentRef={contentRef} />,
  );
  selectIn("article p");
  fireEvent.keyUp(content, { key: "ArrowRight", shiftKey: true });
  expect(await screen.findByTestId("interactions")).toHaveTextContent(
    "keyboard",
  );
});
