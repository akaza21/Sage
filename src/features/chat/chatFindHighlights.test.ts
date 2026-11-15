import { afterEach, expect, it, vi } from "vitest";
import { chatMatchRanges, publishChatHighlights } from "./chatFindHighlights";
import type { ChatMatch } from "./chatFindModel";

afterEach(() => {
  document.body.replaceChildren();
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
});

function match(itemId: string, segment: number, start: number, end: number) {
  return {
    key: `${itemId}:${segment}:${start}`,
    turnId: "turn",
    itemId,
    segment,
    start,
    end,
  } satisfies ChatMatch;
}

it("maps a match crossing markdown nodes without including chrome or another pane", () => {
  document.body.innerHTML =
    '<div id="one"><article class="acp-message" data-item-id="answer"><header>not searchable</header><div class="acp-markdown"><p>A <strong>bold</strong> label</p></div></article></div><div id="two"><article class="acp-message" data-item-id="answer"><div class="acp-markdown"><p>Other pane</p></div></article></div>';
  const ranges = chatMatchRanges(document.getElementById("one")!, [
    match("answer", 0, 2, 12),
  ]);
  expect(ranges.get("answer:0:2")?.toString()).toBe("bold label");
});

it("indexes list items, table cells and code in render order", () => {
  document.body.innerHTML =
    '<article class="acp-message" data-item-id="answer"><div class="acp-markdown"><ul><li><span data-markdown-inline="">one</span></li><li><span data-markdown-inline="">two</span></li></ul><div class="markdown-table"><table><thead><tr><th>head</th></tr></thead><tbody><tr><td>cell</td></tr></tbody></table></div><div><section><pre><code>code</code></pre></section></div><p>Chat preview truncated</p></div></article>';
  const ranges = chatMatchRanges(document.body, [
    match("answer", 1, 0, 3),
    match("answer", 3, 0, 4),
    match("answer", 4, 0, 4),
  ]);
  expect([...ranges.values()].map(String)).toEqual(["two", "cell", "code"]);
});

it("skips unmounted messages and looks each message up once", () => {
  document.body.innerHTML =
    '<article class="acp-message" data-item-id="answer"><div class="acp-markdown"><p>hit hit hit</p></div></article>';
  const lookups = vi.spyOn(Element.prototype, "querySelectorAll");
  const ranges = chatMatchRanges(document.body, [
    match("answer", 0, 0, 3),
    match("answer", 0, 4, 7),
    match("answer", 0, 8, 11),
    match("virtualised", 0, 0, 3),
  ]);
  expect(ranges.size).toBe(3);
  expect(lookups).toHaveBeenCalledTimes(2);
});

it("does not treat lazy diagram status or errors as searchable answer segments", () => {
  document.body.innerHTML =
    '<article class="acp-message" data-item-id="answer"><div class="acp-markdown"><div><p role="status">Loading diagram</p></div><section role="alert"><p>Retry diagram</p></section><p>Needle result</p></div></article>';
  expect(
    chatMatchRanges(document.body, [match("answer", 0, 0, 6)])
      .get("answer:0:0")
      ?.toString(),
  ).toBe("Needle");
});

it("keeps two pane highlight owners independent and releases only its own ranges", () => {
  const highlights = new Map();
  vi.stubGlobal("CSS", { highlights });
  vi.stubGlobal(
    "Highlight",
    class extends Set {
      constructor(...ranges: Range[]) {
        super(ranges);
      }
      priority = 0;
    },
  );
  const first = Symbol();
  const second = Symbol();
  const range = document.createRange();
  publishChatHighlights(first, { other: [range], current: [range] });
  publishChatHighlights(second, { other: [range], current: [] });
  publishChatHighlights(Symbol(), {
    other: [document.createRange()],
    current: [],
  });
  expect(highlights.get("sage-find").size).toBe(1);
  expect(highlights.get("sage-find-current").priority).toBe(1);
  publishChatHighlights(first, null);
  expect(highlights.has("sage-find")).toBe(true);
  expect(highlights.has("sage-find-current")).toBe(false);
  publishChatHighlights(second, null);
  expect(highlights.size).toBe(0);
});
