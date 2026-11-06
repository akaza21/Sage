import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
} from "@testing-library/react";
import {
  Profiler,
  useCallback,
  useRef,
  useState,
  type ProfilerOnRenderCallback,
} from "react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";

import type { TurnProjection } from "../../../domain/acp";
import { useFollowEnd } from "../useFollowEnd";
import { page } from "./acpProjection.fixture";
import { VirtualTurnList } from "./VirtualTurnList";
import type { TurnListHost } from "../chatScroll";
import { useTurnListNavigation } from "../useTurnListNavigation";

const rowHeights = new Map<string, number>();
const observers = new Set<ResizeObserverCallback>();
let scrollTop = 0;

function height(element: Element): number {
  if (element.classList.contains("acp-virtual-turn-row"))
    return (
      rowHeights.get(
        element.firstElementChild!.getAttribute("data-turn-id")!,
      ) ?? 72
    );
  return Number.parseFloat((element as HTMLElement).style.height) || 0;
}

function scrollHeight() {
  const list = document.querySelector(".acp-virtual-turns");
  return list
    ? [...list.children].reduce(
        (total, child) =>
          total +
          height(child) +
          (child.classList.contains("acp-virtual-turn-row") ? 24 : 0),
        24,
      )
    : 400;
}

beforeEach(() => {
  vi.useFakeTimers();
  scrollTop = 0;
  rowHeights.clear();
  observers.clear();
  vi.stubGlobal(
    "ResizeObserver",
    class {
      constructor(private callback: ResizeObserverCallback) {
        observers.add(callback);
      }
      observe() {}
      disconnect() {
        observers.delete(this.callback);
      }
    },
  );
  vi.spyOn(HTMLElement.prototype, "clientHeight", "get").mockReturnValue(400);
  vi.spyOn(HTMLElement.prototype, "scrollHeight", "get").mockImplementation(
    scrollHeight,
  );
  vi.spyOn(HTMLElement.prototype, "scrollTop", "get").mockImplementation(
    () => scrollTop,
  );
  vi.spyOn(HTMLElement.prototype, "scrollTop", "set").mockImplementation(
    (value) => {
      scrollTop = Math.max(0, Math.min(value, scrollHeight() - 400));
    },
  );
  vi.spyOn(HTMLElement.prototype, "getBoundingClientRect").mockImplementation(
    function (this: HTMLElement) {
      if (this.classList.contains("message-list"))
        return new DOMRect(0, 0, 700, 400);
      const row = this.closest(".acp-virtual-turn-row");
      if (row) {
        let top = 12 - scrollTop;
        for (const child of row.parentElement!.children) {
          if (child === row) break;
          top +=
            height(child) +
            (child.classList.contains("acp-virtual-turn-row") ? 24 : 0);
        }
        return new DOMRect(0, top, 700, height(row));
      }
      return new DOMRect(
        0,
        this.classList.contains("acp-virtual-turns") ? 12 - scrollTop : 0,
        700,
        400,
      );
    },
  );
});

afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
  vi.useRealTimers();
});

function turns(first = 1, count = 100) {
  const turn = page("completed").turns[0]!;
  return Array.from({ length: count }, (_, index) => ({
    ...turn,
    id: ("turn-" + (first + index)) as TurnProjection["id"],
    ordinal: first + index,
  }));
}

it("marks only the true final Turn for footer hover time, not a window's last row", () => {
  render(<History turns={turns()} />);
  scrollTo(scrollHeight());
  const last = document.querySelector("[data-last-turn]");
  expect(last).not.toBeNull();
  expect(last).toHaveAttribute("aria-posinset", "100");
  expect(last).toHaveAttribute("aria-setsize", "100");
  scrollTo(0);
  expect(document.querySelector("[data-last-turn]")).toBeNull();
});

function History({
  turns,
  onCommit,
}: {
  turns: readonly TurnProjection[];
  onCommit?: ProfilerOnRenderCallback;
}) {
  const transcriptRef = useRef<HTMLDivElement>(null);
  const contentRef = useRef<HTMLDivElement>(null);
  const endRef = useRef<HTMLDivElement>(null);
  const hostRef = useRef<TurnListHost | null>(null);
  const [scrollport, setScrollport] = useState<HTMLDivElement | null>(null);
  const bindTranscript = useCallback((node: HTMLDivElement | null) => {
    transcriptRef.current = node;
    setScrollport(node);
  }, []);
  // The lazy ChatNavigation chunk owns jump anchoring over the list's host.
  const navigationRef = useTurnListNavigation(scrollport, hostRef, turns);
  const { jumpToLatest, updateFollowEnd } = useFollowEnd({
    ownerKey: "chat-1",
    revision: "1",
    transcriptRef,
    contentRef,
    endRef,
  });
  return (
    <>
      <div
        ref={bindTranscript}
        className="message-list"
        onScroll={updateFollowEnd}
      >
        <div ref={contentRef}>
          <Profiler id="turn-list" onRender={onCommit ?? (() => {})}>
            <VirtualTurnList
              navigationRef={hostRef}
              turns={turns}
              renderTurn={(turn) => (
                <article className="acp-turn" data-turn-id={turn.id}>
                  {turn.id}
                  <p>{streamedText(turn)}</p>
                </article>
              )}
            />
          </Profiler>
          <div ref={endRef} />
        </div>
      </div>
      <button
        onClick={() => navigationRef.current?.scrollToTurn("turn-12", "auto")}
      >
        Go to retained turn
      </button>
      <button
        onClick={() => {
          jumpToLatest();
          fireEvent.scroll(transcriptRef.current!);
        }}
      >
        Jump to latest
      </button>
    </>
  );
}

function visibleAnchor() {
  const row = [...document.querySelectorAll<HTMLElement>(".acp-turn")].find(
    (element) => element.getBoundingClientRect().bottom > 16,
  )!;
  return { id: row.dataset.turnId, top: row.getBoundingClientRect().top };
}

it("mounts a requested retained Turn at 12px and preserves it through resize and prepend", () => {
  const current = turns();
  const view = render(<History turns={current} />);
  scrollTo(scrollHeight());
  expect(document.querySelector('[data-turn-id="turn-12"]')).toBeNull();
  fireEvent.click(screen.getByRole("button", { name: "Go to retained turn" }));
  act(() => vi.advanceTimersByTime(50));
  expect(visibleAnchor()).toEqual({ id: "turn-12", top: 12 });
  act(() => fireEvent.scroll(document.querySelector(".message-list")!));
  rowHeights.set("turn-11", 240);
  act(() => {
    for (const observer of observers) observer([], {} as ResizeObserver);
  });
  view.rerender(<History turns={[...turns(-9, 10), ...current]} />);
  act(() => vi.advanceTimersByTime(50));
  expect(visibleAnchor()).toEqual({ id: "turn-12", top: 12 });
  scrollTo(0);
  expect(visibleAnchor().id).not.toBe("turn-12");
});

function scrollTo(top: number) {
  const transcript = document.querySelector<HTMLElement>(".message-list")!;
  act(() => {
    fireEvent.wheel(transcript);
    transcript.scrollTop = top;
    fireEvent.scroll(transcript);
  });
  act(() => vi.advanceTimersByTime(50));
}

it("Jump to latest lands at the true end after short Turn estimates are measured", () => {
  render(<History turns={turns()} />);
  scrollTo(0);
  fireEvent.click(screen.getByRole("button", { name: "Jump to latest" }));
  act(() => vi.advanceTimersByTime(400));
  // jsdom fires no scroll event for the timed jump's own scrollTop writes.
  fireEvent.scroll(document.querySelector(".message-list")!);
  act(() => vi.advanceTimersByTime(50));
  expect(scrollHeight() - scrollTop - 400).toBeLessThanOrEqual(1);
  expect(document.querySelector('[data-turn-id="turn-100"]')).not.toBeNull();
});

it("retains the settled anchor after a correction scroll event and another history prepend", () => {
  const current = turns();
  const view = render(<History turns={current} />);
  scrollTo(scrollHeight() / 2);
  const before = visibleAnchor();
  const older = [...turns(-9, 10), ...current];
  view.rerender(<History turns={older} />);
  expect(visibleAnchor()).toEqual(before);
  expect(document.querySelectorAll(".acp-turn").length).toBeLessThanOrEqual(40);
  act(() => fireEvent.scroll(document.querySelector(".message-list")!));
  act(() => vi.advanceTimersByTime(50));
  view.rerender(<History turns={[...turns(-19, 10), ...older]} />);
  expect(visibleAnchor()).toEqual(before);
});

it("keeps the same visible Turn when a measured overscan row grows", () => {
  render(<History turns={turns()} />);
  scrollTo(scrollHeight() / 2);
  const before = visibleAnchor();
  const first = document.querySelector<HTMLElement>(".acp-turn")!;
  expect(first.dataset.turnId).not.toBe(before.id);
  rowHeights.set(first.dataset.turnId!, 360);
  act(() => {
    for (const observer of [...observers]) observer([], {} as ResizeObserver);
  });
  expect(visibleAnchor()).toEqual(before);
});

function streamedText(turn: TurnProjection) {
  const [item] = turn.items;
  return item?.kind === "text" ? item.payload.text : "";
}

function withText(turn: TurnProjection, text: string, revision: number) {
  const [item] = turn.items;
  return {
    ...turn,
    revision: String(revision) as TurnProjection["revision"],
    items: [{ ...item, payload: { role: "assistant", text } }],
  } as TurnProjection;
}

it("streams each delta into the pinned final Turn in one commit, staying at the end", () => {
  let current = turns();
  let commits = 0;
  const view = render(
    <History turns={current} onCommit={() => (commits += 1)} />,
  );
  scrollTo(scrollHeight());
  const pinnedTop = scrollTop;
  expect(scrollHeight() - pinnedTop - 400).toBeLessThanOrEqual(1);
  commits = 0;

  let streamed = "";
  for (let delta = 1; delta <= 30; delta += 1) {
    streamed += `delta ${delta} `;
    current = [
      ...current.slice(0, -1),
      withText(current.at(-1)!, streamed, delta + 1),
    ];
    view.rerender(<History turns={current} onCommit={() => (commits += 1)} />);

    const last = document.querySelector('[data-turn-id="turn-100"]');
    expect(last).toHaveTextContent(streamed.trim());
    // The row keeps its height, so following the end leaves the scroll put.
    expect(scrollTop).toBe(pinnedTop);
    // No extra layout or measurement pass rides along with a delta.
    expect(commits).toBe(delta);
  }
  act(() => vi.advanceTimersByTime(50));
  expect(scrollHeight() - scrollTop - 400).toBeLessThanOrEqual(1);
  expect(
    document.querySelector('[data-turn-id="turn-100"]')!.getBoundingClientRect()
      .bottom,
  ).toBeLessThanOrEqual(400);
});
