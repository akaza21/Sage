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

function mountFollowEnd() {
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
    ({ revision, ownerKey }: { revision: number; ownerKey: string }) =>
      useFollowEnd({
        ownerKey,
        revision,
        transcriptRef,
        contentRef,
        endRef,
      }),
    { initialProps: { revision: 1, ownerKey: "chat-1" } },
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
    act(() => {
      view.result.current.updateFollowEnd();
      frames.shift()?.(0);
    });
    expect(view.result.current.followingEnd).toBe(true);
    expect(view.result.current.showJumpToLatest).toBe(false);
  });

  it("restores Jump to latest when the reader interrupts smooth travel", () => {
    const frames: FrameRequestCallback[] = [];
    vi.stubGlobal("requestAnimationFrame", (callback: FrameRequestCallback) => {
      frames.push(callback);
      return frames.length;
    });
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
      transcript.dispatchEvent(new WheelEvent("wheel", { bubbles: true })),
    );
    expect(view.result.current.followingEnd).toBe(false);
    expect(view.result.current.showJumpToLatest).toBe(true);
  });

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
});
