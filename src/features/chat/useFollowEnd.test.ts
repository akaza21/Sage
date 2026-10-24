import { readFileSync } from "node:fs";
import { act, renderHook } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { useFollowEnd } from "./useFollowEnd";

let observers: { callback: ResizeObserverCallback; targets: Element[] }[] = [];

class ResizeObserverMock {
  private readonly record: {
    callback: ResizeObserverCallback;
    targets: Element[];
  };

  constructor(callback: ResizeObserverCallback) {
    this.record = { callback, targets: [] };
    observers.push(this.record);
  }

  observe(target: Element) {
    this.record.targets.push(target);
  }

  unobserve() {}

  disconnect() {
    observers = observers.filter((entry) => entry !== this.record);
  }
}

function growTranscript() {
  for (const observer of [...observers]) {
    observer.callback([], {} as ResizeObserver);
  }
}

function measure(
  element: HTMLElement,
  metrics: { clientHeight: number; scrollHeight: number; scrollTop: number },
) {
  for (const [key, value] of Object.entries(metrics)) {
    Object.defineProperty(element, key, { configurable: true, value });
  }
}

function mountFollowEnd(running = false) {
  const transcript = document.createElement("div");
  const content = document.createElement("div");
  const end = document.createElement("div");
  transcript.append(content, end);
  document.body.append(transcript);
  measure(transcript, { clientHeight: 400, scrollHeight: 400, scrollTop: 0 });

  const scrollTo = vi.fn();
  const scrollIntoView = vi.fn();
  transcript.scrollTo = scrollTo as unknown as HTMLElement["scrollTo"];
  end.scrollIntoView = scrollIntoView;

  const transcriptRef = { current: transcript as HTMLElement | null };
  const contentRef = { current: content as HTMLElement | null };
  const endRef = { current: end as HTMLElement | null };

  const view = renderHook(
    ({
      revision,
      ownerKey,
      live = running,
    }: {
      revision: number;
      ownerKey: string;
      live?: boolean;
    }) =>
      useFollowEnd({
        ownerKey,
        revision,
        running: live,
        transcriptRef,
        contentRef,
        endRef,
      }),
    {
      initialProps: { revision: 1, ownerKey: "chat-1" } as {
        revision: number;
        ownerKey: string;
        live?: boolean;
      },
    },
  );

  return { content, end, scrollIntoView, scrollTo, transcript, view };
}

describe("useFollowEnd", () => {
  beforeEach(() => {
    observers = [];
    vi.stubGlobal("ResizeObserver", ResizeObserverMock);
    vi.stubGlobal("requestAnimationFrame", (callback: FrameRequestCallback) => {
      callback(0);
      return 1;
    });
    vi.stubGlobal("cancelAnimationFrame", () => {});
  });

  afterEach(() => {
    vi.unstubAllGlobals();
    document.body.replaceChildren();
  });

  it("keeps reader navigation paused even when the selected match is already at the end", () => {
    const { transcript, view, scrollTo } = mountFollowEnd();
    act(() => transcript.dispatchEvent(new Event("sage-chat-navigation")));
    act(() => view.result.current.updateFollowEnd());
    expect(view.result.current.followingEnd).toBe(false);
    scrollTo.mockClear();
    measure(transcript, {
      clientHeight: 400,
      scrollHeight: 1000,
      scrollTop: 0,
    });
    view.rerender({ revision: 2, ownerKey: "chat-1" });
    expect(scrollTo).not.toHaveBeenCalled();
    act(() => view.result.current.jumpToLatest());
    expect(view.result.current.followingEnd).toBe(true);
  });

  it("scrolls the transcript to its last scrollable pixel rather than the end marker", () => {
    const { scrollIntoView, scrollTo, transcript, view } = mountFollowEnd();
    scrollTo.mockClear();

    measure(transcript, {
      clientHeight: 400,
      scrollHeight: 2400,
      scrollTop: 0,
    });
    act(() => view.result.current.jumpToLatest());

    expect(scrollTo).toHaveBeenCalledWith(
      expect.objectContaining({ top: 2000 }),
    );
    expect(scrollIntoView).not.toHaveBeenCalled();
  });

  it("falls back to the end marker only when there is no transcript", () => {
    const transcript = document.createElement("div");
    const content = document.createElement("div");
    const end = document.createElement("div");
    transcript.append(content, end);
    const scrollIntoView = vi.fn();
    end.scrollIntoView = scrollIntoView;

    renderHook(() =>
      useFollowEnd({
        ownerKey: "chat-1",
        revision: 1,
        transcriptRef: { current: null },
        contentRef: { current: content },
        endRef: { current: end },
      }),
    );

    expect(scrollIntoView).toHaveBeenCalledWith(
      expect.objectContaining({ block: "end" }),
    );
  });

  it("re-anchors to the end when late layout leaves the transcript short of it", () => {
    const { scrollTo, transcript } = mountFollowEnd();
    scrollTo.mockClear();

    measure(transcript, {
      clientHeight: 400,
      scrollHeight: 2400,
      scrollTop: 0,
    });
    act(() => growTranscript());

    expect(scrollTo).toHaveBeenCalledWith({ behavior: "auto", top: 2000 });
  });

  it("leaves the transcript alone when growth keeps it at the end", () => {
    const { scrollTo, transcript } = mountFollowEnd();
    scrollTo.mockClear();

    measure(transcript, {
      clientHeight: 400,
      scrollHeight: 2400,
      scrollTop: 2000,
    });
    act(() => growTranscript());

    expect(scrollTo).not.toHaveBeenCalled();
  });

  it("observes the transcript and its content wrapper with one observer", () => {
    const { content, transcript } = mountFollowEnd();

    expect(observers).toHaveLength(1);
    expect(observers[0]?.targets).toEqual([transcript, content]);
  });

  it("keeps the same observer across revisions and follow-state changes", () => {
    const { transcript, view } = mountFollowEnd();
    const created = observers[0];

    view.rerender({ revision: 2, ownerKey: "chat-1" });
    measure(transcript, {
      clientHeight: 400,
      scrollHeight: 2400,
      scrollTop: 0,
    });
    act(() => view.result.current.updateFollowEnd());
    view.rerender({ revision: 3, ownerKey: "chat-1" });

    expect(view.result.current.followingEnd).toBe(false);
    expect(observers).toHaveLength(1);
    expect(observers[0]).toBe(created);
  });

  it("stops re-anchoring once the reader scrolls away from the end", () => {
    const { scrollTo, transcript, view } = mountFollowEnd();

    measure(transcript, {
      clientHeight: 400,
      scrollHeight: 2400,
      scrollTop: 0,
    });
    act(() => view.result.current.updateFollowEnd());
    expect(view.result.current.followingEnd).toBe(false);
    scrollTo.mockClear();

    act(() => growTranscript());

    expect(scrollTo).not.toHaveBeenCalled();
  });

  it("offers Jump to latest only beyond one visible transcript height", () => {
    const near = mountFollowEnd();
    measure(near.transcript, {
      clientHeight: 400,
      scrollHeight: 2400,
      scrollTop: 1700,
    });
    act(() => near.view.result.current.updateFollowEnd());
    expect(near.view.result.current.followingEnd).toBe(false);
    expect(near.view.result.current.showJumpToLatest).toBe(false);

    const far = mountFollowEnd();
    measure(far.transcript, {
      clientHeight: 400,
      scrollHeight: 2400,
      scrollTop: 1599,
    });
    act(() => far.view.result.current.updateFollowEnd());
    expect(far.view.result.current.showJumpToLatest).toBe(true);

    act(() => far.view.result.current.jumpToLatest());
    expect(far.view.result.current.showJumpToLatest).toBe(false);
  });

  it("does not carry a prior Chat's jump pill into a new owner", () => {
    const { transcript, view } = mountFollowEnd();
    measure(transcript, {
      clientHeight: 400,
      scrollHeight: 2400,
      scrollTop: 100,
    });
    act(() => view.result.current.updateFollowEnd());
    expect(view.result.current.showJumpToLatest).toBe(true);

    measure(transcript, {
      clientHeight: 400,
      scrollHeight: 400,
      scrollTop: 0,
    });
    view.rerender({ revision: 2, ownerKey: "chat-2" });
    expect(view.result.current.showJumpToLatest).toBe(false);
    expect(view.result.current.followingEnd).toBe(true);
  });

  it("keeps the pill hidden while a smooth jump is still travelling", () => {
    const frames: FrameRequestCallback[] = [];
    vi.stubGlobal("requestAnimationFrame", (callback: FrameRequestCallback) => {
      frames.push(callback);
      return frames.length;
    });
    const { transcript, view, scrollTo } = mountFollowEnd();
    measure(transcript, {
      clientHeight: 400,
      scrollHeight: 2400,
      scrollTop: 100,
    });
    act(() => {
      view.result.current.updateFollowEnd();
      frames.shift()?.(0);
    });
    expect(view.result.current.showJumpToLatest).toBe(true);

    act(() => view.result.current.jumpToLatest());
    expect(view.result.current.showJumpToLatest).toBe(false);
    measure(transcript, {
      clientHeight: 400,
      scrollHeight: 2400,
      scrollTop: 900,
    });
    act(() => {
      view.result.current.updateFollowEnd();
      frames.shift()?.(0);
    });
    expect(view.result.current.showJumpToLatest).toBe(false);

    measure(transcript, {
      clientHeight: 400,
      scrollHeight: 2400,
      scrollTop: 2000,
    });
    expect(scrollTo).not.toHaveBeenCalledWith(
      expect.objectContaining({ behavior: "auto" }),
    );
    act(() => {
      view.result.current.updateFollowEnd();
      frames.shift()?.(0);
    });
    expect(view.result.current.followingEnd).toBe(true);
    expect(view.result.current.showJumpToLatest).toBe(false);
  });

  it.each(["wheel", "pointerdown", "touchstart", "keydown"])(
    "restores Jump to latest when %s interrupts smooth travel",
    (eventType) => {
      const frames: FrameRequestCallback[] = [];
      vi.stubGlobal(
        "requestAnimationFrame",
        (callback: FrameRequestCallback) => {
          frames.push(callback);
          return frames.length;
        },
      );
      const { transcript, view } = mountFollowEnd();
      measure(transcript, {
        clientHeight: 400,
        scrollHeight: 2400,
        scrollTop: 100,
      });
      act(() => {
        view.result.current.updateFollowEnd();
        frames.shift()?.(0);
      });
      act(() => view.result.current.jumpToLatest());
      measure(transcript, {
        clientHeight: 400,
        scrollHeight: 2400,
        scrollTop: 800,
      });
      act(() =>
        transcript.dispatchEvent(new Event(eventType, { bubbles: true })),
      );
      expect(view.result.current.followingEnd).toBe(false);
      expect(view.result.current.showJumpToLatest).toBe(true);
    },
  );

  it("leaves the end on an upward wheel before its scroll moves past the threshold", () => {
    const frames: FrameRequestCallback[] = [];
    vi.stubGlobal("requestAnimationFrame", (callback: FrameRequestCallback) => {
      frames.push(callback);
      return frames.length;
    });
    const { scrollTo, transcript, view } = mountFollowEnd(true);
    // At the end of a transcript that can still scroll up.
    measure(transcript, {
      clientHeight: 400,
      scrollHeight: 1000,
      scrollTop: 600,
    });
    act(() =>
      transcript.dispatchEvent(
        new WheelEvent("wheel", { deltaY: -240, bubbles: true }),
      ),
    );
    // The smooth wheel scroll has barely moved when activity lands.
    act(() => {
      view.result.current.updateFollowEnd();
      frames.shift()?.(0);
    });
    expect(view.result.current.followingEnd).toBe(false);
    scrollTo.mockClear();
    measure(transcript, {
      clientHeight: 400,
      scrollHeight: 1500,
      scrollTop: 600,
    });
    view.rerender({ revision: 2, ownerKey: "chat-1" });
    act(() => growTranscript());
    expect(scrollTo).not.toHaveBeenCalled();
    expect(view.result.current.showJumpToLatest).toBe("activity");
    // A downward wheel back at the end resumes following.
    act(() =>
      transcript.dispatchEvent(
        new WheelEvent("wheel", { deltaY: 240, bubbles: true }),
      ),
    );
    measure(transcript, {
      clientHeight: 400,
      scrollHeight: 1500,
      scrollTop: 1100,
    });
    act(() => {
      view.result.current.updateFollowEnd();
      frames.shift()?.(0);
    });
    expect(view.result.current.followingEnd).toBe(true);
  });

  it("keeps following when an upward wheel scrolls a nested scroller instead", () => {
    const { content, scrollTo, transcript, view } = mountFollowEnd(true);
    measure(transcript, {
      clientHeight: 400,
      scrollHeight: 1000,
      scrollTop: 600,
    });
    const rollup = document.createElement("div");
    rollup.className = "acp-tool-rollup__items";
    const row = document.createElement("div");
    rollup.append(row);
    content.append(rollup);
    measure(rollup, { clientHeight: 100, scrollHeight: 300, scrollTop: 50 });
    act(() =>
      row.dispatchEvent(
        new WheelEvent("wheel", { deltaY: -120, bubbles: true }),
      ),
    );
    expect(view.result.current.followingEnd).toBe(true);
    scrollTo.mockClear();
    measure(transcript, {
      clientHeight: 400,
      scrollHeight: 1500,
      scrollTop: 600,
    });
    view.rerender({ revision: 2, ownerKey: "chat-1" });
    expect(scrollTo).toHaveBeenCalled();
  });

  it("keeps following when an upward wheel meets a short chat", () => {
    const { scrollTo, transcript, view } = mountFollowEnd(true);
    measure(transcript, { clientHeight: 400, scrollHeight: 400, scrollTop: 0 });
    act(() =>
      transcript.dispatchEvent(
        new WheelEvent("wheel", { deltaY: -240, bubbles: true }),
      ),
    );
    expect(view.result.current.followingEnd).toBe(true);
    scrollTo.mockClear();
    measure(transcript, { clientHeight: 400, scrollHeight: 900, scrollTop: 0 });
    view.rerender({ revision: 2, ownerKey: "chat-1" });
    expect(scrollTo).toHaveBeenCalled();
  });

  it("keeps following when its own anchor scroll steps through a stale target", () => {
    const frames: FrameRequestCallback[] = [];
    vi.stubGlobal("requestAnimationFrame", (callback: FrameRequestCallback) => {
      frames.push(callback);
      return frames.length;
    });
    const { scrollTo, transcript, view } = mountFollowEnd();
    // A new revision anchors toward the end of the wide layout.
    measure(transcript, { clientHeight: 633, scrollHeight: 649, scrollTop: 0 });
    view.rerender({ revision: 2, ownerKey: "chat-1" });
    expect(scrollTo).toHaveBeenLastCalledWith(
      expect.objectContaining({ top: 16 }),
    );
    // Layout settles within the follow threshold before the travel starts.
    act(() => growTranscript());

    measure(transcript, { clientHeight: 633, scrollHeight: 649, scrollTop: 7 });
    act(() => {
      view.result.current.updateFollowEnd();
      frames.shift()?.(0);
    });
    expect(view.result.current.followingEnd).toBe(true);

    // The pane narrows before that travel lands; its first scroll step reports
    // a position more than the follow threshold away from the new end.
    measure(transcript, { clientHeight: 617, scrollHeight: 700, scrollTop: 7 });
    act(() => {
      view.result.current.updateFollowEnd();
      frames.shift()?.(0);
    });
    expect(view.result.current.followingEnd).toBe(true);

    scrollTo.mockClear();
    act(() => growTranscript());
    expect(scrollTo).toHaveBeenCalledWith({ behavior: "auto", top: 83 });
  });

  it.each(["wheel", "pointerdown", "touchstart", "keydown"])(
    "lets %s inside the transcript interrupt an anchor scroll",
    (eventType) => {
      const { content, scrollTo, transcript, view } = mountFollowEnd();
      measure(transcript, {
        clientHeight: 400,
        scrollHeight: 2400,
        scrollTop: 0,
      });
      view.rerender({ revision: 2, ownerKey: "chat-1" });

      act(() => content.dispatchEvent(new Event(eventType, { bubbles: true })));
      act(() => view.result.current.updateFollowEnd());
      expect(view.result.current.followingEnd).toBe(false);

      scrollTo.mockClear();
      act(() => growTranscript());
      expect(scrollTo).not.toHaveBeenCalled();
    },
  );

  it("treats a scroll back toward the start during an anchor as the reader's", () => {
    const frames: FrameRequestCallback[] = [];
    vi.stubGlobal("requestAnimationFrame", (callback: FrameRequestCallback) => {
      frames.push(callback);
      return frames.length;
    });
    const { transcript, view } = mountFollowEnd();
    measure(transcript, {
      clientHeight: 400,
      scrollHeight: 2400,
      scrollTop: 0,
    });
    view.rerender({ revision: 2, ownerKey: "chat-1" });
    measure(transcript, {
      clientHeight: 400,
      scrollHeight: 2400,
      scrollTop: 900,
    });
    act(() => {
      view.result.current.updateFollowEnd();
      frames.shift()?.(0);
    });
    expect(view.result.current.followingEnd).toBe(true);

    measure(transcript, {
      clientHeight: 400,
      scrollHeight: 2400,
      scrollTop: 0,
    });
    act(() => {
      view.result.current.updateFollowEnd();
      frames.shift()?.(0);
    });
    expect(view.result.current.followingEnd).toBe(false);
    expect(view.result.current.showJumpToLatest).toBe(true);
  });

  it("lands at the new true end when measurement interrupts a smooth jump", () => {
    const frames: FrameRequestCallback[] = [];
    vi.stubGlobal("requestAnimationFrame", (callback: FrameRequestCallback) => {
      frames.push(callback);
      return frames.length;
    });
    const { transcript, view, scrollTo } = mountFollowEnd();
    measure(transcript, {
      clientHeight: 400,
      scrollHeight: 2400,
      scrollTop: 100,
    });
    act(() => view.result.current.updateFollowEnd());
    act(() => view.result.current.jumpToLatest());
    expect(scrollTo).toHaveBeenLastCalledWith({
      behavior: "smooth",
      top: 2000,
    });
    measure(transcript, {
      clientHeight: 400,
      scrollHeight: 2520,
      scrollTop: 900,
    });
    scrollTo.mockImplementation(({ behavior, top }: ScrollToOptions) => {
      if (behavior === "auto")
        measure(transcript, {
          clientHeight: 400,
          scrollHeight: 2520,
          scrollTop: top!,
        });
    });
    act(growTranscript);
    expect(
      transcript.scrollHeight - transcript.scrollTop - transcript.clientHeight,
    ).toBe(0);
    expect(view.result.current.showJumpToLatest).toBe(false);
  });

  it("finishes a stalled smooth jump without an aggregate content resize", () => {
    const frames: FrameRequestCallback[] = [];
    vi.stubGlobal("requestAnimationFrame", (callback: FrameRequestCallback) => {
      frames.push(callback);
      return frames.length;
    });
    const { transcript, view, scrollTo } = mountFollowEnd();
    measure(transcript, {
      clientHeight: 400,
      scrollHeight: 2400,
      scrollTop: 100,
    });
    act(() => view.result.current.jumpToLatest());
    expect(scrollTo).toHaveBeenLastCalledWith({
      behavior: "smooth",
      top: 2000,
    });
    measure(transcript, {
      clientHeight: 400,
      scrollHeight: 2400,
      scrollTop: 900,
    });
    scrollTo.mockImplementation(({ behavior, top }: ScrollToOptions) => {
      if (behavior === "auto")
        measure(transcript, {
          clientHeight: 400,
          scrollHeight: 2400,
          scrollTop: top!,
        });
    });
    act(() => view.result.current.updateFollowEnd());
    for (let frame = 0; frame < 3; frame += 1)
      act(() => frames.shift()?.(frame * 16));
    expect(
      transcript.scrollHeight - transcript.scrollTop - transcript.clientHeight,
    ).toBe(0);
    expect(view.result.current.showJumpToLatest).toBe(false);
  });

  it.each([900, 2000])(
    "reconciles a reader scroll to %s immediately after interrupting a jump",
    (scrollTop) => {
      const frames = new Map<number, FrameRequestCallback>();
      let nextFrame = 0;
      vi.stubGlobal(
        "requestAnimationFrame",
        (callback: FrameRequestCallback) => {
          frames.set(++nextFrame, callback);
          return nextFrame;
        },
      );
      vi.stubGlobal("cancelAnimationFrame", (frame: number) =>
        frames.delete(frame),
      );
      const { transcript, view } = mountFollowEnd();
      measure(transcript, {
        clientHeight: 400,
        scrollHeight: 2400,
        scrollTop: 1700,
      });
      act(() => view.result.current.jumpToLatest());
      act(() => transcript.dispatchEvent(new Event("wheel")));
      expect(view.result.current.showJumpToLatest).toBe(false);
      measure(transcript, { clientHeight: 400, scrollHeight: 2400, scrollTop });
      act(() => view.result.current.updateFollowEnd());
      act(() => {
        for (const [frame, callback] of [...frames]) {
          frames.delete(frame);
          callback(16);
        }
      });
      expect(view.result.current.followingEnd).toBe(scrollTop === 2000);
      expect(view.result.current.showJumpToLatest).toBe(scrollTop === 900);
    },
  );

  it("does not re-show the pill from resize while a jump begins", () => {
    const { transcript, view } = mountFollowEnd();
    measure(transcript, {
      clientHeight: 400,
      scrollHeight: 2400,
      scrollTop: 100,
    });
    act(() => view.result.current.updateFollowEnd());
    expect(view.result.current.showJumpToLatest).toBe(true);
    act(() => {
      view.result.current.jumpToLatest();
      growTranscript();
    });
    expect(view.result.current.showJumpToLatest).toBe(false);
  });

  it.each([1999, 2000])(
    "releases an anchor when it lands at %s",
    (scrollTop) => {
      const frames: FrameRequestCallback[] = [];
      vi.stubGlobal(
        "requestAnimationFrame",
        (callback: FrameRequestCallback) => {
          frames.push(callback);
          return frames.length;
        },
      );
      const { transcript, view } = mountFollowEnd();
      measure(transcript, {
        clientHeight: 400,
        scrollHeight: 2400,
        scrollTop: 0,
      });
      view.rerender({ revision: 2, ownerKey: "chat-1" });
      measure(transcript, { clientHeight: 400, scrollHeight: 2400, scrollTop });
      act(() => {
        view.result.current.updateFollowEnd();
        frames.shift()?.(0);
      });

      measure(transcript, {
        clientHeight: 400,
        scrollHeight: 3400,
        scrollTop: 2100,
      });
      act(() => {
        view.result.current.updateFollowEnd();
        frames.shift()?.(0);
      });
      expect(view.result.current.followingEnd).toBe(false);
      expect(view.result.current.showJumpToLatest).toBe(true);
    },
  );

  it("does not carry an old owner's travelling anchor into a new Chat", () => {
    const frames: FrameRequestCallback[] = [];
    vi.stubGlobal("requestAnimationFrame", (callback: FrameRequestCallback) => {
      frames.push(callback);
      return frames.length;
    });
    const { transcript, view } = mountFollowEnd();
    measure(transcript, {
      clientHeight: 400,
      scrollHeight: 2400,
      scrollTop: 0,
    });
    view.rerender({ revision: 2, ownerKey: "chat-1" });
    measure(transcript, {
      clientHeight: 400,
      scrollHeight: 2400,
      scrollTop: 900,
    });
    act(() => {
      view.result.current.updateFollowEnd();
      frames.shift()?.(0);
    });

    measure(transcript, { clientHeight: 400, scrollHeight: 400, scrollTop: 0 });
    view.rerender({ revision: 2, ownerKey: "chat-2" });
    measure(transcript, {
      clientHeight: 400,
      scrollHeight: 2400,
      scrollTop: 100,
    });
    act(() => {
      view.result.current.updateFollowEnd();
      frames.shift()?.(0);
    });
    expect(view.result.current.followingEnd).toBe(false);
    expect(view.result.current.showJumpToLatest).toBe(true);
  });

  it("compares anchor travel against the new owner's scroll position", () => {
    const frames: FrameRequestCallback[] = [];
    vi.stubGlobal("requestAnimationFrame", (callback: FrameRequestCallback) => {
      frames.push(callback);
      return frames.length;
    });
    const { scrollTo, transcript, view } = mountFollowEnd();
    measure(transcript, {
      clientHeight: 400,
      scrollHeight: 2400,
      scrollTop: 0,
    });
    view.rerender({ revision: 2, ownerKey: "chat-1" });
    measure(transcript, {
      clientHeight: 400,
      scrollHeight: 2400,
      scrollTop: 900,
    });
    act(() => {
      view.result.current.updateFollowEnd();
      frames.shift()?.(0);
    });

    measure(transcript, {
      clientHeight: 400,
      scrollHeight: 2400,
      scrollTop: 0,
    });
    scrollTo.mockClear();
    view.rerender({ revision: 3, ownerKey: "chat-2" });
    expect(scrollTo).toHaveBeenCalledWith(
      expect.objectContaining({ top: 2000 }),
    );
    measure(transcript, {
      clientHeight: 400,
      scrollHeight: 2400,
      scrollTop: 100,
    });
    act(() => {
      view.result.current.updateFollowEnd();
      frames.shift()?.(0);
    });
    expect(view.result.current.followingEnd).toBe(true);
    scrollTo.mockClear();
    act(() => growTranscript());
    expect(scrollTo).toHaveBeenCalledWith({ behavior: "auto", top: 2000 });
  });

  it("opts the transcript scroller out of browser scroll anchoring", () => {
    const sheet = document.createElement("style");
    sheet.textContent = readFileSync("src/styles/chat-surface.css", "utf8");
    document.body.append(sheet);
    const { transcript } = mountFollowEnd();
    transcript.className = "message-list";

    expect(
      getComputedStyle(transcript).getPropertyValue("overflow-anchor"),
    ).toBe("none");
  });
  /** Follows a 1000 px transcript, then the reader scrolls 100 px up. */
  function pauseNearEnd(running: boolean) {
    const frames: FrameRequestCallback[] = [];
    vi.stubGlobal("requestAnimationFrame", (callback: FrameRequestCallback) => {
      frames.push(callback);
      return frames.length;
    });
    const mounted = mountFollowEnd(running);
    const at = (scrollTop: number, scrollHeight: number) =>
      measure(mounted.transcript, {
        clientHeight: 400,
        scrollHeight,
        scrollTop,
      });
    const readerScroll = (scrollTop: number, scrollHeight: number) => {
      at(scrollTop, scrollHeight);
      act(() => {
        mounted.view.result.current.updateFollowEnd();
        for (const frame of frames.splice(0)) frame(0);
      });
    };
    const grow = (scrollTop: number, scrollHeight: number) => {
      at(scrollTop, scrollHeight);
      act(() => growTranscript());
    };
    grow(600, 1000);
    readerScroll(500, 1000);
    expect(mounted.view.result.current.followingEnd).toBe(false);
    expect(mounted.view.result.current.showJumpToLatest).toBe(false);
    return { ...mounted, grow, readerScroll };
  }

  it("shows the new-activity pill when a running turn grows below a paused reader", () => {
    const { grow, view, scrollTo } = pauseNearEnd(true);
    expect(view.result.current.showJumpToLatest).not.toBe("activity");
    scrollTo.mockClear();

    grow(500, 1100);

    expect(view.result.current.showJumpToLatest).toBe("activity");
    expect(scrollTo).not.toHaveBeenCalled();
  });

  it("keeps the pill through reader scrolls and clears it on jump to latest", () => {
    const { grow, readerScroll, view, scrollTo } = pauseNearEnd(true);
    grow(500, 1100);
    readerScroll(450, 1100);
    expect(view.result.current.showJumpToLatest).toBe("activity");

    scrollTo.mockClear();
    act(() => view.result.current.jumpToLatest());
    expect(scrollTo).toHaveBeenCalledWith(
      expect.objectContaining({ top: 700 }),
    );
    expect(view.result.current.showJumpToLatest).not.toBe("activity");
    expect(view.result.current.showJumpToLatest).toBe(false);
  });

  it("does not show the pill for growth outside a running turn", () => {
    const { grow, view } = pauseNearEnd(false);
    grow(500, 1100);
    expect(view.result.current.showJumpToLatest).not.toBe("activity");
    expect(view.result.current.showJumpToLatest).toBe(false);
  });

  it("stops showing the pill once the turn ends", () => {
    const { grow, view } = pauseNearEnd(true);
    view.rerender({ revision: 2, ownerKey: "chat-1", live: false });
    grow(500, 1100);
    expect(view.result.current.showJumpToLatest).not.toBe("activity");
  });

  it("does not show the pill while the reader still follows the end", () => {
    const { transcript, view } = mountFollowEnd(true);
    measure(transcript, {
      clientHeight: 400,
      scrollHeight: 1000,
      scrollTop: 600,
    });
    act(() => growTranscript());
    measure(transcript, {
      clientHeight: 400,
      scrollHeight: 1100,
      scrollTop: 700,
    });
    act(() => growTranscript());
    expect(view.result.current.followingEnd).toBe(true);
    expect(view.result.current.showJumpToLatest).not.toBe("activity");
  });

  it("clears the pill when the reader scrolls back to the end", () => {
    const { grow, readerScroll, view } = pauseNearEnd(true);
    grow(500, 1100);
    expect(view.result.current.showJumpToLatest).toBe("activity");
    readerScroll(700, 1100);
    expect(view.result.current.followingEnd).toBe(true);
    expect(view.result.current.showJumpToLatest).not.toBe("activity");
  });
});
