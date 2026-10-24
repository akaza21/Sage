import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { emptyChatOwnershipEntry } from "../../app/chatOwnershipProjection";
import { useWorkbenchStore } from "../../app/store";
import {
  attachContextToDraft,
  type AttachedContext,
} from "../../domain/attachedContext";

const side = vi.hoisted(() => ({ open: vi.fn() }));
vi.mock("../../workbench/sideChatActions", () => ({
  openSideChat: side.open,
}));

import { requestAnnotation } from "./transcriptSelection";
import { TranscriptInteractions } from "./TranscriptInteractions";

const initialState = useWorkbenchStore.getState();
let scrollport: HTMLElement;
let content: HTMLElement;
let composer: HTMLTextAreaElement;

function contexts(): readonly AttachedContext[] {
  return useWorkbenchStore.getState().chatUiById.c?.attachedContext ?? [];
}

beforeEach(() => {
  side.open.mockResolvedValue(undefined);
  scrollport = document.createElement("div");
  content = document.createElement("div");
  content.innerHTML =
    '<article data-selectable-answer="t" data-item-id="i" tabindex="0"><p>Null becomes text when greeted.</p></article>';
  scrollport.append(content);
  vi.spyOn(scrollport, "getBoundingClientRect").mockReturnValue({
    left: 0,
    top: 0,
    width: 800,
    bottom: 600,
  } as DOMRect);
  composer = document.createElement("textarea");
  composer.id = "prompt-c";
  document.body.append(scrollport, composer);
  vi.spyOn(Range.prototype, "getClientRects").mockReturnValue([
    { left: 180, top: 100, bottom: 121, right: 230 },
  ] as unknown as DOMRectList);
  useWorkbenchStore.setState({
    projectTransitioning: false,
    chatUiById: { c: emptyChatOwnershipEntry() },
    captureChatOwnership: (() => ({
      chatId: "c",
    })) as unknown as typeof initialState.captureChatOwnership,
    attachChatContext: (_intent, context) => {
      const entry = useWorkbenchStore.getState().chatUiById.c;
      useWorkbenchStore.setState({
        chatUiById: {
          c: {
            ...entry,
            attachedContext: attachContextToDraft(
              entry.attachedContext,
              context,
            ),
          },
        },
      });
      return true;
    },
  });
});

afterEach(() => {
  cleanup();
  window.getSelection()?.removeAllRanges();
  scrollport.remove();
  composer.remove();
  useWorkbenchStore.setState(initialState, true);
});

function selectWord(start: number, end: number) {
  const node = content.querySelector("p")!.firstChild!;
  const range = document.createRange();
  range.setStart(node, start);
  range.setEnd(node, end);
  window.getSelection()!.removeAllRanges();
  window.getSelection()!.addRange(range);
  fireEvent.pointerUp(content);
}

function mount() {
  return render(
    <TranscriptInteractions
      chatId="c"
      focused
      contentRef={{ current: content }}
      initialKeyboard={false}
    />,
  );
}

it("Add to chat creates a numbered annotation and opens its comment field", async () => {
  mount();
  selectWord(0, 4);
  fireEvent.click(await screen.findByRole("button", { name: "Add to chat" }));
  expect(contexts()).toEqual([
    expect.objectContaining({
      kind: "transcriptQuote",
      ordinal: 1,
      text: "Null",
      sourceTurnId: "t",
      sourceItemId: "i",
    }),
  ]);
  const field = await screen.findByRole("textbox", {
    name: "Annotation comment",
  });
  expect(field).toHaveFocus();
  expect(screen.getByRole("button", { name: "Annotation 1" })).toBeVisible();
  expect(screen.queryByRole("toolbar")).toBeNull();
  fireEvent.change(field, { target: { value: "Is this true for null?" } });
  fireEvent.submit(field.closest("form")!);
  await waitFor(() =>
    expect(
      screen.queryByRole("textbox", { name: "Annotation comment" }),
    ).toBeNull(),
  );
  expect(contexts()[0]).toMatchObject({ comment: "Is this true for null?" });
  expect(composer).toHaveFocus();
  expect(screen.getByRole("button", { name: "Annotation 1" })).toBeVisible();
});

it("refuses a ninth annotation with the exact status", async () => {
  useWorkbenchStore.setState({
    chatUiById: {
      c: {
        ...emptyChatOwnershipEntry(),
        attachedContext: Array.from({ length: 8 }, (_, index) => ({
          kind: "transcriptQuote" as const,
          ordinal: index + 1,
          sourceTurnId: "t",
          sourceItemId: `other-${index}`,
          text: "x",
          startOffset: 0,
          endOffset: 1,
        })),
      },
    },
  });
  mount();
  selectWord(0, 4);
  fireEvent.click(await screen.findByRole("button", { name: "Add to chat" }));
  expect(await screen.findByRole("status")).toHaveTextContent(
    "Up to 8 annotations per message",
  );
  expect(contexts()).toHaveLength(8);
});

it("reopens the comment field for the chip's edit request and Escape keeps the comment", async () => {
  useWorkbenchStore.setState({
    chatUiById: {
      c: {
        ...emptyChatOwnershipEntry(),
        attachedContext: [
          {
            kind: "transcriptQuote",
            ordinal: 1,
            sourceTurnId: "t",
            sourceItemId: "i",
            text: "Null",
            comment: "Earlier",
            startOffset: 0,
            endOffset: 4,
          },
        ],
      },
    },
  });
  mount();
  act(() => requestAnnotation("edit", "c", "quote:i:0-4"));
  const field = await screen.findByRole("textbox", {
    name: "Annotation comment",
  });
  expect(field).toHaveValue("Earlier");
  fireEvent.keyDown(field, { key: "Escape" });
  await waitFor(() =>
    expect(
      screen.queryByRole("textbox", { name: "Annotation comment" }),
    ).toBeNull(),
  );
  expect(contexts()[0]).toMatchObject({ comment: "Earlier" });
});

it("Ask in side chat hands the bounded quote to the side-chat action", async () => {
  mount();
  selectWord(0, 4);
  fireEvent.click(
    await screen.findByRole("button", { name: "Ask in side chat" }),
  );
  expect(side.open).toHaveBeenCalledWith(
    "c",
    expect.objectContaining({ text: "Null", turnId: "t", itemId: "i" }),
  );
  expect(contexts()).toHaveLength(0);
});
