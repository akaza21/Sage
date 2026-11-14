import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
  within,
} from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import { useRef, useState } from "react";
import { ChatFindController } from "./ChatFindController";
import { chatFindStatus } from "./chatFindModel";
import { page, textItem } from "./acp/acpProjection.fixture";
import type { TurnListNavigation } from "./chatScroll";
import type { TurnProjection } from "../../domain/acp";

afterEach(() => {
  cleanup();
  vi.useRealTimers();
});

it("uses a 10px find search icon (R3-12c)", () => {
  render(<Pane name="Chat" />);
  fireEvent.keyDown(screen.getByRole("textbox", { name: "Chat composer" }), {
    key: "f",
    ctrlKey: true,
  });
  expect(
    screen.getByRole("search").querySelector(".chat-find__icon"),
  ).toHaveAttribute("width", "10");
});

function turnsWith(text: string): TurnProjection[] {
  return [
    {
      ...page("completed").turns[0]!,
      items: [
        {
          ...textItem(),
          origin: "primary" as const,
          payload: { role: "assistant" as const, text },
        },
      ],
    },
  ];
}

function Pane({
  name,
  controller,
  turns = turnsWith("hit HIT hit"),
}: {
  name: string;
  controller?: TurnListNavigation;
  turns?: TurnProjection[];
}) {
  const navigation = useRef<TurnListNavigation | null>(controller ?? null);
  const [port, setPort] = useState<HTMLDivElement | null>(null);
  return (
    <section className="chat-surface" aria-label={name}>
      <div className="message-list" ref={setPort}>
        <p>plain transcript text</p>
      </div>
      <form className="composer">
        <textarea aria-label={`${name} composer`} />
      </form>
      {port ? (
        <ChatFindController
          turns={turns}
          scrollport={port}
          navigationRef={navigation}
          incomplete
        />
      ) : null}
    </section>
  );
}

async function openFind(target: Element) {
  fireEvent.keyDown(target, { key: "f", ctrlKey: true });
  await act(async () => {});
}

it("debounces, wraps, closes to its composer, and leaves editor and other pane Ctrl+F alone", async () => {
  vi.useFakeTimers();
  render(
    <>
      <Pane name="Alpha" />
      <Pane name="Beta" />
      <input aria-label="Editor" />
    </>,
  );
  const alpha = screen.getByRole("region", { name: "Alpha" });
  const composer = screen.getByRole("textbox", { name: "Alpha composer" });
  composer.focus();
  await openFind(composer);
  const search = within(alpha).getByRole("searchbox", { name: "Search chat" });
  expect(search).toHaveFocus();
  expect(search).toHaveAttribute("placeholder", "Search chat…");
  expect(screen.getAllByRole("searchbox")).toHaveLength(1);
  expect(within(alpha).queryByRole("status")).not.toBeInTheDocument();
  fireEvent.change(search, { target: { value: "hit" } });
  expect(within(alpha).getByRole("status")).toHaveTextContent("0 results");
  expect(
    within(alpha).getByRole("button", { name: "Next match" }),
  ).toBeDisabled();
  act(() => vi.advanceTimersByTime(199));
  expect(within(alpha).getByRole("status")).toHaveTextContent("0 results");
  act(() => vi.advanceTimersByTime(1));
  expect(within(alpha).getByRole("status")).toHaveTextContent("1 / 3 results");
  fireEvent.keyDown(search, { key: "Enter", shiftKey: true });
  expect(within(alpha).getByRole("status")).toHaveTextContent("3 / 3 results");
  fireEvent.keyDown(search, { key: "Enter" });
  expect(within(alpha).getByRole("status")).toHaveTextContent("1 / 3 results");
  fireEvent.keyDown(search, { key: "F3" });
  expect(within(alpha).getByRole("status")).toHaveTextContent("2 / 3 results");
  fireEvent.click(
    within(alpha).getByRole("button", { name: "Previous match" }),
  );
  expect(within(alpha).getByRole("status")).toHaveTextContent("1 / 3 results");
  expect(within(alpha).getByText(/Only loaded turns/)).toBeInTheDocument();
  fireEvent.keyDown(search, { key: "Escape" });
  expect(screen.queryByRole("searchbox")).not.toBeInTheDocument();
  expect(composer).toHaveFocus();
  const editor = screen.getByRole("textbox", { name: "Editor" });
  expect(fireEvent.keyDown(editor, { key: "f", ctrlKey: true })).toBe(true);
  const codeMirror = document.createElement("div");
  codeMirror.className = "cm-editor";
  alpha.append(codeMirror);
  expect(fireEvent.keyDown(codeMirror, { key: "f", ctrlKey: true })).toBe(true);
});

it("sends Ctrl+F on an unfocused page to the pane the pointer last pressed", async () => {
  render(
    <>
      <Pane name="Alpha" />
      <Pane name="Beta" />
    </>,
  );
  const beta = screen.getByRole("region", { name: "Beta" });
  fireEvent.pointerDown(within(beta).getByText("plain transcript text"));
  (document.activeElement as HTMLElement | null)?.blur();
  await openFind(document.body);
  expect(
    within(beta).getByRole("searchbox", { name: "Search chat" }),
  ).toHaveFocus();
  expect(screen.getAllByRole("searchbox")).toHaveLength(1);
});

it("keeps the selected match when streaming recomputes the matches", async () => {
  vi.useFakeTimers();
  const view = render(<Pane name="Alpha" turns={turnsWith("hit hit")} />);
  await openFind(screen.getByRole("textbox", { name: "Alpha composer" }));
  const search = screen.getByRole("searchbox", { name: "Search chat" });
  fireEvent.change(search, { target: { value: "hit" } });
  act(() => vi.advanceTimersByTime(200));
  fireEvent.keyDown(search, { key: "Enter" });
  expect(screen.getByRole("status")).toHaveTextContent("2 / 2 results");
  view.rerender(<Pane name="Alpha" turns={turnsWith("hit hit hit")} />);
  expect(screen.getByRole("status")).toHaveTextContent("2 / 3 results");
});

it("jumps by index to a match in an unmounted virtual turn, and again on a repeat step", async () => {
  vi.useFakeTimers();
  const controller = {
    scrollToTurn: vi.fn(),
    anchorTurn: vi.fn(),
    visibleTurnIds: () => [],
  };
  render(<Pane name="Alpha" controller={controller} />);
  await openFind(screen.getByRole("textbox", { name: "Alpha composer" }));
  const search = screen.getByRole("searchbox", { name: "Search chat" });
  fireEvent.change(search, { target: { value: "hit HIT hit" } });
  act(() => vi.advanceTimersByTime(200));
  expect(controller.scrollToTurn).toHaveBeenCalledTimes(1);
  expect(controller.scrollToTurn).toHaveBeenLastCalledWith(
    expect.any(String),
    "auto",
  );
  expect(screen.getByRole("status")).toHaveTextContent("1 result");
  fireEvent.keyDown(search, { key: "Enter" });
  expect(controller.scrollToTurn).toHaveBeenCalledTimes(2);
});

it("words the status like Codex", () => {
  const base = { pending: false, index: 0, capped: false };
  expect(chatFindStatus({ ...base, count: 0 })).toBe("0 results");
  expect(chatFindStatus({ ...base, pending: true, count: 4 })).toBe(
    "0 results",
  );
  expect(chatFindStatus({ ...base, count: 1 })).toBe("1 result");
  expect(chatFindStatus({ ...base, index: 1, count: 6 })).toBe("2 / 6 results");
  expect(chatFindStatus({ ...base, capped: true, count: 500 })).toBe(
    "1 / 500+ results",
  );
});

function RemountingPane({
  mounted,
  chatId = "alpha",
}: {
  mounted: boolean;
  chatId?: string;
}) {
  const navigation = useRef<TurnListNavigation | null>(null);
  const [port, setPort] = useState<HTMLDivElement | null>(null);
  return (
    <section className="chat-surface" aria-label="Remounting">
      <div className="message-list" ref={setPort}>
        <p>plain transcript text</p>
      </div>
      <form className="composer">
        <textarea aria-label="Remounting composer" />
      </form>
      {port && mounted ? (
        <ChatFindController
          turns={turnsWith("hit HIT hit")}
          scrollport={port}
          navigationRef={navigation}
          incomplete={false}
          chatId={chatId}
        />
      ) : null}
    </section>
  );
}

it("keeps an open find and its query across a transcript remount in the same pane", async () => {
  vi.useFakeTimers();
  const { rerender } = render(<RemountingPane mounted />);
  const composer = screen.getByRole("textbox", { name: "Remounting composer" });
  composer.focus();
  await openFind(composer);
  fireEvent.change(screen.getByRole("searchbox", { name: "Search chat" }), {
    target: { value: "hit" },
  });
  act(() => vi.advanceTimersByTime(200));
  expect(screen.getByRole("status")).toHaveTextContent("1 / 3 results");
  rerender(<RemountingPane mounted={false} />);
  rerender(<RemountingPane mounted />);
  const search = screen.getByRole("searchbox", { name: "Search chat" });
  expect(search).toHaveValue("hit");
  expect(screen.getByRole("status")).toHaveTextContent("1 / 3 results");
  expect(search).not.toHaveFocus();
  rerender(<RemountingPane mounted={false} />);
  rerender(<RemountingPane mounted chatId="beta" />);
  expect(screen.queryByRole("searchbox")).not.toBeInTheDocument();
  rerender(<RemountingPane mounted={false} />);
  rerender(<RemountingPane mounted />);
  expect(screen.queryByRole("searchbox")).not.toBeInTheDocument();
  await openFind(screen.getByRole("textbox", { name: "Remounting composer" }));
  fireEvent.keyDown(screen.getByRole("searchbox", { name: "Search chat" }), {
    key: "Escape",
  });
  rerender(<RemountingPane mounted={false} />);
  rerender(<RemountingPane mounted />);
  expect(screen.queryByRole("searchbox")).not.toBeInTheDocument();
});
