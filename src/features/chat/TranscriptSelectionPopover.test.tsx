import {
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { TranscriptSelectionPopover } from "./TranscriptSelectionPopover";

let scrollport: HTMLElement;
let content: HTMLElement;

beforeEach(() => {
  scrollport = document.createElement("div");
  content = document.createElement("div");
  content.innerHTML =
    '<article data-selectable-answer="t" data-item-id="i" tabindex="0"><header>Agent</header><p>A selected answer</p></article>' +
    '<article data-selectable-answer="u" data-item-id="j" tabindex="0"><p>Second answer</p></article>' +
    '<article data-item-id="user"><p>User bubble</p></article>';
  scrollport.append(content);
  document.body.append(scrollport);
  vi.spyOn(Range.prototype, "getClientRects").mockReturnValue([
    { left: 180, top: 100, bottom: 121, right: 230 },
  ] as unknown as DOMRectList);
});

afterEach(() => {
  cleanup();
  window.getSelection()?.removeAllRanges();
  scrollport.remove();
});

function select(start: Node, startOffset: number, end = start, endOffset = -1) {
  const range = document.createRange();
  range.setStart(start, startOffset);
  range.setEnd(end, endOffset < 0 ? (end.textContent?.length ?? 0) : endOffset);
  window.getSelection()!.removeAllRanges();
  window.getSelection()!.addRange(range);
}

const text = (selector: string) =>
  content.querySelector(selector)!.firstChild as Text;

function mount(onAdd = vi.fn(), onSideChat = vi.fn()) {
  render(
    <TranscriptSelectionPopover
      content={content}
      scrollport={scrollport}
      onAdd={onAdd}
      onSideChat={onSideChat}
    />,
  );
  return { onAdd, onSideChat };
}

it("offers the exact Codex actions on mouse-up, positioned above the selection", async () => {
  const { onAdd } = mount();
  select(text('[data-item-id="i"] p'), 2, text('[data-item-id="i"] p'), 10);
  fireEvent.pointerUp(content);
  const toolbar = await screen.findByRole("toolbar", {
    name: "Selection actions",
  });
  expect(
    [...toolbar.querySelectorAll("button")].map((button) => button.textContent),
  ).toEqual(["Add to chat", "Ask in side chat"]);
  fireEvent.click(screen.getByRole("button", { name: "Add to chat" }));
  expect(onAdd).toHaveBeenCalledWith({
    turnId: "t",
    itemId: "i",
    text: "selected",
    startOffset: 2,
    endOffset: 10,
  });
  expect(screen.queryByRole("toolbar")).toBeNull();
  expect(window.getSelection()?.isCollapsed).toBe(true);
});

it.each([
  [
    "two answers",
    () =>
      select(text('[data-item-id="i"] p'), 0, text('[data-item-id="j"] p'), 3),
  ],
  ["a user bubble", () => select(text('[data-item-id="user"] p'), 0)],
  [
    "whitespace only",
    () => {
      content.querySelector('[data-item-id="j"] p')!.textContent = "   ";
      select(text('[data-item-id="j"] p'), 0);
    },
  ],
  ["answer chrome", () => select(text("header"), 0)],
])("renders nothing for a selection spanning %s", (_label, arrange) => {
  mount();
  arrange();
  fireEvent.pointerUp(content);
  expect(screen.queryByRole("toolbar")).toBeNull();
});

it("dismisses on Escape (restoring focus), scroll, outside pointer and collapse", async () => {
  mount();
  const answer = content.querySelector<HTMLElement>('[data-item-id="i"]')!;
  answer.focus();
  const show = async () => {
    select(text('[data-item-id="i"] p'), 2, text('[data-item-id="i"] p'), 10);
    fireEvent.pointerUp(content);
    return screen.findByRole("toolbar");
  };
  await show();
  fireEvent.keyDown(document, { key: "Escape" });
  await waitFor(() => expect(screen.queryByRole("toolbar")).toBeNull());
  expect(answer).toHaveFocus();
  await show();
  fireEvent.scroll(scrollport);
  await waitFor(() => expect(screen.queryByRole("toolbar")).toBeNull());
  await show();
  fireEvent.pointerDown(document.body);
  await waitFor(() => expect(screen.queryByRole("toolbar")).toBeNull());
  await show();
  window.getSelection()!.removeAllRanges();
  fireEvent(document, new Event("selectionchange"));
  await waitFor(() => expect(screen.queryByRole("toolbar")).toBeNull());
});

it.each([{ key: "F10", shiftKey: true }, { key: "ContextMenu" }])(
  "opens from $key without a mouse, quoting the focused answer",
  async (event) => {
    const { onSideChat } = mount();
    const answer = content.querySelector<HTMLElement>('[data-item-id="i"]')!;
    answer.focus();
    fireEvent.keyDown(answer, event);
    expect(
      await screen.findByRole("button", { name: "Add to chat" }),
    ).toHaveFocus();
    fireEvent.keyDown(screen.getByRole("toolbar"), { key: "ArrowRight" });
    expect(
      screen.getByRole("button", { name: "Ask in side chat" }),
    ).toHaveFocus();
    fireEvent.click(screen.getByRole("button", { name: "Ask in side chat" }));
    expect(onSideChat).toHaveBeenCalledWith(
      expect.objectContaining({ text: "A selected answer", itemId: "i" }),
    );
  },
);

it("attaches dismiss listeners only while shown and detaches everything on unmount", async () => {
  const documentAdd = vi.spyOn(document, "addEventListener");
  const contentAdd = vi.spyOn(content, "addEventListener");
  const contentRemove = vi.spyOn(content, "removeEventListener");
  const documentRemove = vi.spyOn(document, "removeEventListener");
  const { unmount } = render(
    <TranscriptSelectionPopover
      content={content}
      scrollport={scrollport}
      onAdd={vi.fn()}
      onSideChat={vi.fn()}
    />,
  );
  const selectionListeners = () =>
    documentAdd.mock.calls.filter(([type]) => type === "selectionchange");
  expect(selectionListeners()).toHaveLength(0);
  select(text('[data-item-id="i"] p'), 2, text('[data-item-id="i"] p'), 10);
  fireEvent.pointerUp(content);
  await screen.findByRole("toolbar");
  expect(selectionListeners()).toHaveLength(1);
  unmount();
  expect(contentRemove.mock.calls.length).toBe(contentAdd.mock.calls.length);
  expect(
    documentRemove.mock.calls.filter(([type]) => type === "selectionchange"),
  ).toHaveLength(1);
});

it("centres on the selection start and overhangs the column inside the scrollport (s2/f0093)", async () => {
  const answer = content.querySelector<HTMLElement>('[data-item-id="i"]')!;
  const box = (left: number, width: number) =>
    ({
      left,
      top: 0,
      width,
      height: 28,
      right: left + width,
      bottom: 28,
    }) as DOMRect;
  vi.spyOn(HTMLElement.prototype, "getBoundingClientRect").mockImplementation(
    function (this: HTMLElement) {
      if (this === scrollport) return box(0, 600);
      if (this === answer) return box(160, 300);
      return box(0, this.getAttribute("role") === "toolbar" ? 172 : 0);
    },
  );
  Object.defineProperty(scrollport, "clientWidth", { value: 600 });
  mount();
  select(text('[data-item-id="i"] p'), 2, text('[data-item-id="i"] p'), 10);
  fireEvent.pointerUp(content);
  const toolbar = await screen.findByRole("toolbar");
  // 180 - 172 / 2: left of the 160 px answer column, not clamped to it.
  await waitFor(() => expect(toolbar.style.left).toBe("94px"));
});
