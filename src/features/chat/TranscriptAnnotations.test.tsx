import { cleanup, render, screen, waitFor } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import { attachedContextKey } from "../../domain/attachedContext";
import { TranscriptAnnotations } from "./TranscriptAnnotations";

afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
});

it("highlights only the open comment, keeps its badge after confirmation, and cleans up on send", async () => {
  const registry = new Map<string, { ranges: Range[] }>();
  vi.stubGlobal("CSS", { highlights: registry });
  vi.stubGlobal(
    "Highlight",
    class {
      ranges: Range[];
      constructor(...ranges: Range[]) {
        this.ranges = ranges;
      }
    },
  );
  const content = document.createElement("div");
  content.innerHTML =
    '<div data-selectable-answer="turn" data-item-id="answer">quoted text</div>';
  const scrollport = document.createElement("div");
  scrollport.append(content);
  document.body.append(scrollport);
  vi.spyOn(scrollport, "getBoundingClientRect").mockReturnValue({
    top: 0,
    bottom: 600,
  } as DOMRect);
  vi.spyOn(Range.prototype, "getClientRects").mockReturnValue([
    { left: 100, right: 170, top: 80, bottom: 101 },
  ] as unknown as DOMRectList);
  const quote = {
    kind: "transcriptQuote" as const,
    ordinal: 1,
    sourceTurnId: "turn",
    sourceItemId: "answer",
    text: "quoted text",
    startOffset: 0,
    endOffset: 11,
  };
  const onOpen = vi.fn();
  const props = {
    content,
    scrollport,
    quotes: [quote],
    editingKey: attachedContextKey(quote),
    onOpen,
  };
  const { rerender, unmount } = render(<TranscriptAnnotations {...props} />);
  expect(
    await screen.findByRole("button", { name: "Annotation 1" }),
  ).toBeVisible();
  expect([...registry.values()][0].ranges).toHaveLength(1);
  rerender(<TranscriptAnnotations {...props} editingKey={null} />);
  await waitFor(() => expect([...registry.values()][0].ranges).toHaveLength(0));
  expect(screen.getByRole("button", { name: "Annotation 1" })).toBeVisible();
  screen.getByRole("button", { name: "Annotation 1" }).click();
  expect(onOpen).toHaveBeenCalledOnce();
  rerender(<TranscriptAnnotations {...props} quotes={[]} editingKey={null} />);
  expect(screen.queryByRole("button", { name: "Annotation 1" })).toBeNull();
  unmount();
  expect(registry.size).toBe(0);
  scrollport.remove();
});

it("keeps a badge when Custom Highlight is unavailable", async () => {
  vi.stubGlobal("CSS", {});
  vi.stubGlobal("Highlight", undefined);
  const content = document.createElement("div");
  content.innerHTML =
    '<div data-selectable-answer="turn" data-item-id="answer">quote</div>';
  document.body.append(content);
  vi.spyOn(content, "getBoundingClientRect").mockReturnValue({
    top: 0,
    bottom: 600,
  } as DOMRect);
  vi.spyOn(Range.prototype, "getClientRects").mockReturnValue([
    { left: 100, right: 170, top: 80, bottom: 101 },
  ] as unknown as DOMRectList);
  render(
    <TranscriptAnnotations
      content={content}
      scrollport={content}
      quotes={[
        {
          kind: "transcriptQuote",
          ordinal: 1,
          sourceTurnId: "turn",
          sourceItemId: "answer",
          text: "quote",
          startOffset: 0,
          endOffset: 5,
        },
      ]}
      editingKey={null}
      onOpen={vi.fn()}
    />,
  );
  expect(
    await screen.findByRole("button", { name: "Annotation 1" }),
  ).toBeVisible();
});

it("re-resolves only the quoted answer whose subtree changed, at most once per frame", async () => {
  vi.stubGlobal("CSS", {});
  vi.stubGlobal("Highlight", undefined);
  const frames: FrameRequestCallback[] = [];
  vi.stubGlobal("requestAnimationFrame", (callback: FrameRequestCallback) => {
    frames.push(callback);
    return frames.length;
  });
  vi.stubGlobal("cancelAnimationFrame", () => undefined);
  const flush = () => {
    for (const callback of frames.splice(0)) callback(0);
  };
  const content = document.createElement("div");
  content.innerHTML =
    '<div data-selectable-answer="t1" data-item-id="a">alpha text</div>' +
    '<div data-selectable-answer="t2" data-item-id="b">bravo text</div>' +
    '<p id="elsewhere">streaming</p>';
  document.body.append(content);
  vi.spyOn(content, "getBoundingClientRect").mockReturnValue({
    top: 0,
    bottom: 600,
  } as DOMRect);
  vi.spyOn(Range.prototype, "getClientRects").mockReturnValue([
    { left: 100, right: 170, top: 80, bottom: 101 },
  ] as unknown as DOMRectList);
  const lookups = vi.spyOn(content, "querySelectorAll");
  const quote = (turn: string, item: string, text: string) => ({
    kind: "transcriptQuote" as const,
    ordinal: 1,
    sourceTurnId: turn,
    sourceItemId: item,
    text,
    startOffset: 0,
    endOffset: text.length,
  });
  render(
    <TranscriptAnnotations
      content={content}
      scrollport={content}
      quotes={[quote("t1", "a", "alpha"), quote("t2", "b", "bravo")]}
      editingKey={null}
      onOpen={vi.fn()}
    />,
  );
  expect(
    await screen.findByRole("button", { name: "Annotation 2" }),
  ).toBeVisible();
  const resolved = lookups.mock.calls.length;
  content.querySelector("#elsewhere")!.textContent = "streaming more";
  await new Promise((settle) => setTimeout(settle, 0));
  flush();
  content.dispatchEvent(new Event("scroll"));
  flush();
  expect(lookups.mock.calls.length).toBe(resolved);
  const bravo = content.querySelector('[data-item-id="b"]')!;
  bravo.firstChild!.textContent = "bravo text!";
  bravo.append(" more");
  await new Promise((settle) => setTimeout(settle, 0));
  expect(frames.length).toBeLessThanOrEqual(1);
  const ranges = vi.spyOn(document, "createRange");
  flush();
  expect(ranges).toHaveBeenCalledTimes(1);
  expect(screen.getByRole("button", { name: "Annotation 1" })).toBeVisible();
  content.remove();
});

it("waits for an unmounted answer without lookups, then follows it when it mounts or is replaced", async () => {
  vi.stubGlobal("CSS", {});
  vi.stubGlobal("Highlight", undefined);
  const frames: FrameRequestCallback[] = [];
  vi.stubGlobal("requestAnimationFrame", (callback: FrameRequestCallback) => {
    frames.push(callback);
    return frames.length;
  });
  vi.stubGlobal("cancelAnimationFrame", () => undefined);
  const flush = async () => {
    await new Promise((settle) => setTimeout(settle, 0));
    for (const callback of frames.splice(0)) callback(0);
  };
  const content = document.createElement("div");
  content.innerHTML = '<div class="list"></div>';
  document.body.append(content);
  vi.spyOn(content, "getBoundingClientRect").mockReturnValue({
    top: 0,
    bottom: 600,
  } as DOMRect);
  vi.spyOn(Range.prototype, "getClientRects").mockReturnValue([
    { left: 100, right: 170, top: 80, bottom: 101 },
  ] as unknown as DOMRectList);
  const lookups = vi.spyOn(content, "querySelectorAll");
  render(
    <TranscriptAnnotations
      content={content}
      scrollport={content}
      quotes={[
        {
          kind: "transcriptQuote",
          ordinal: 1,
          sourceTurnId: "t",
          sourceItemId: "a",
          text: "alpha",
          startOffset: 0,
          endOffset: 5,
        },
      ]}
      editingKey={null}
      onOpen={vi.fn()}
    />,
  );
  const initial = lookups.mock.calls.length;
  content.dispatchEvent(new Event("scroll"));
  await flush();
  expect(lookups.mock.calls.length).toBe(initial);
  const list = content.querySelector(".list")!;
  const answer = () => {
    const element = document.createElement("div");
    element.dataset.selectableAnswer = "t";
    element.dataset.itemId = "a";
    element.textContent = "alpha text";
    return element;
  };
  list.append(answer());
  await flush();
  expect(
    await screen.findByRole("button", { name: "Annotation 1" }),
  ).toBeVisible();
  list.replaceChildren(answer());
  await flush();
  expect(screen.getByRole("button", { name: "Annotation 1" })).toBeVisible();
  const resolvedRange = vi.spyOn(document, "createRange");
  list.replaceChildren(answer());
  await flush();
  expect(resolvedRange).toHaveBeenCalledTimes(1);
  content.remove();
});

function frameHarness() {
  vi.stubGlobal("CSS", {});
  vi.stubGlobal("Highlight", undefined);
  const frames: FrameRequestCallback[] = [];
  vi.stubGlobal("requestAnimationFrame", (callback: FrameRequestCallback) => {
    frames.push(callback);
    return frames.length;
  });
  vi.stubGlobal("cancelAnimationFrame", () => undefined);
  const resized: (() => void)[] = [];
  vi.stubGlobal(
    "ResizeObserver",
    class {
      constructor(callback: () => void) {
        resized.push(callback);
      }
      observe() {}
      disconnect() {}
    },
  );
  vi.spyOn(Range.prototype, "getClientRects").mockReturnValue([
    { left: 100, right: 170, top: 80, bottom: 101 },
  ] as unknown as DOMRectList);
  return {
    resize: () => resized.forEach((callback) => callback()),
    flush: async () => {
      await new Promise((settle) => setTimeout(settle, 0));
      for (const callback of frames.splice(0)) callback(0);
    },
  };
}

const quoteOf = (turn: string, item: string) => ({
  kind: "transcriptQuote" as const,
  ordinal: 1,
  sourceTurnId: turn,
  sourceItemId: item,
  text: "alpha",
  startOffset: 0,
  endOffset: 5,
});

it("brings a badge back when the virtual list unmounts and remounts the answer's turn row", async () => {
  const { flush } = frameHarness();
  const content = document.createElement("div");
  content.innerHTML =
    '<section class="row"><div><div data-selectable-answer="t" data-item-id="a">alpha text</div></div></section>';
  document.body.append(content);
  vi.spyOn(content, "getBoundingClientRect").mockReturnValue({
    top: 0,
    bottom: 600,
  } as DOMRect);
  render(
    <TranscriptAnnotations
      content={content}
      scrollport={content}
      quotes={[quoteOf("t", "a")]}
      editingKey={null}
      onOpen={vi.fn()}
    />,
  );
  expect(
    await screen.findByRole("button", { name: "Annotation 1" }),
  ).toBeVisible();
  const row = content.querySelector(".row")!;
  row.remove();
  content.dispatchEvent(new Event("scroll"));
  await flush();
  await waitFor(() =>
    expect(screen.queryByRole("button", { name: "Annotation 1" })).toBeNull(),
  );
  content.append(row);
  await flush();
  expect(
    await screen.findByRole("button", { name: "Annotation 1" }),
  ).toBeVisible();
  content.remove();
});

it("never waits on a quote whose answer lives in another Chat", async () => {
  const { flush } = frameHarness();
  const content = document.createElement("div");
  document.body.append(content);
  vi.spyOn(content, "getBoundingClientRect").mockReturnValue({
    top: 0,
    bottom: 600,
  } as DOMRect);
  const observe = vi.spyOn(MutationObserver.prototype, "observe");
  const lookups = vi.spyOn(content, "querySelectorAll");
  render(
    <TranscriptAnnotations
      content={content}
      scrollport={content}
      quotes={[quoteOf("parent-turn", "parent-answer")]}
      editingKey={null}
      onOpen={vi.fn()}
      isForeign={(quote) => quote.sourceTurnId === "parent-turn"}
    />,
  );
  content.append(document.createElement("p"));
  await flush();
  expect(observe.mock.calls.some(([target]) => target === content)).toBe(false);
  expect(lookups).not.toHaveBeenCalled();
  content.remove();
});
