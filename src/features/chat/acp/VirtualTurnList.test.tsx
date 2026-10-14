import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
} from "@testing-library/react";
import { Profiler, useRef, type ProfilerOnRenderCallback } from "react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";

import type { TurnProjection } from "../../../domain/acp";
import { useFollowEnd } from "../useFollowEnd";
import { page } from "./acpProjection.fixture";
import { VirtualTurnList } from "./VirtualTurnList";

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
        ref={transcriptRef}
        className="message-list"
        onScroll={updateFollowEnd}
      >
        <div ref={contentRef}>
          <Profiler id="turn-list" onRender={onCommit ?? (() => {})}>
            <VirtualTurnList
              turns={turns}
              renderTurn={(turn) => (
                <article className="acp-turn" data-turn-id={turn.id}>
                  {turn.id}
                </article>
              )}
            />
          </Profiler>
          <div ref={endRef} />
        </div>
      </div>
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

function scrollTo(top: number) {
  const transcript = document.querySelector<HTMLElement>(".message-list")!;
  act(() => {
    fireEvent.wheel(transcript);
    transcript.scrollTop = top;
    fireEvent.scroll(transcript);
  });
  act(() => vi.advanceTimersByTime(50));
}

it("preserves the measured visible short Turn when earlier Turns arrive", () => {
  const current = turns();
  const view = render(<History turns={current} />);
  scrollTo(scrollHeight() / 2);
  const before = visibleAnchor();
  view.rerender(<History turns={[...turns(-9, 10), ...current]} />);
  expect(visibleAnchor()).toEqual(before);
  expect(document.querySelectorAll(".acp-turn").length).toBeLessThanOrEqual(40);
});

it("Jump to latest lands at the true end after short Turn estimates are measured", () => {
  render(<History turns={turns()} />);
  scrollTo(0);
  fireEvent.click(screen.getByRole("button", { name: "Jump to latest" }));
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

it("keeps height-stable end-pinned streaming to one commit per delta", () => {
  let current = turns();
  const commits = vi.fn();
  const view = render(<History turns={current} onCommit={commits} />);
  scrollTo(scrollHeight());
  commits.mockClear();
  for (let index = 0; index < 30; index += 1) {
    const last = current.at(-1)!;
    current = [
      ...current.slice(0, -1),
      { ...last, revision: String(index + 2) as TurnProjection["revision"] },
    ];
    view.rerender(<History turns={current} onCommit={commits} />);
  }
  expect(commits).toHaveBeenCalledTimes(30);
});
