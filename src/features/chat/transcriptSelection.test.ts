import { describe, expect, it } from "vitest";
import {
  quoteContext,
  quoteRange,
  readTranscriptQuote,
  selectAnswerText,
  selectionPopoverPosition,
} from "./transcriptSelection";

describe("answer projection", () => {
  it("skips hidden meta and code-block chrome, and quotes the focused answer for keyboard users", () => {
    document.body.innerHTML =
      '<div id="content"><article data-selectable-answer="t" data-item-id="a" tabindex="0"><header>Agent 12:23</header><p>Use <code>add</code> here.</p><figure><header><span>JavaScript</span><button>Copy</button></header><pre>add(1)</pre></figure><p class="acp-message__partial">Partial answer retained</p></article></div>';
    const content = document.querySelector<HTMLElement>("#content")!;
    const answer = content.querySelector<HTMLElement>("article")!;
    const quote = selectAnswerText(answer, content)?.quote;
    expect(quote).toEqual({
      turnId: "t",
      itemId: "a",
      text: "Use add here.add(1)",
      startOffset: 0,
      endOffset: 19,
    });
    const context = quoteContext(quote!);
    expect(context).toMatchObject({ kind: "transcriptQuote", ordinal: 1 });
    expect(quoteRange(content, context)?.toString()).toContain("add(1)");
    expect(quoteRange(content, { ...context, text: "moved text" })).toBeNull();
    expect(selectAnswerText(content.querySelector("p")!, content)).toBeNull();
    window.getSelection()!.removeAllRanges();
  });

  it("refuses quotes beyond 4096 UTF-8 bytes", () => {
    document.body.innerHTML = `<div id="content"><article data-selectable-answer="t" data-item-id="a">${"水".repeat(1366)}</article></div>`;
    const content = document.querySelector<HTMLElement>("#content")!;
    expect(
      selectAnswerText(content.querySelector("article")!, content),
    ).toBeNull();
    window.getSelection()!.removeAllRanges();
  });
});

describe("transcript selection", () => {
  it.each([
    [150, 100, 100],
    [5, 100, 8],
    [295, 100, 192],
  ])("clamps selection at %i", (left, width, expected) => {
    expect(
      selectionPopoverPosition(
        { left, top: 100, bottom: 121 },
        { left: 0, top: 0, width: 300 },
        { width, height: 29 },
      ).left,
    ).toBe(expected);
  });
  it.each([
    [332, 388],
    [262, 218],
  ])(
    "keeps the pill 8px inside both pane edges from %ipx (R3-08)",
    (left, width) => {
      const position = (selected: number) =>
        selectionPopoverPosition(
          { left: selected, top: 300, bottom: 321 },
          { left, top: 92, width },
          { width: 188, height: 28 },
        ).left;
      expect(position(left)).toBe(8);
      expect(position(left + width)).toBe(width - 196);
      expect(position(left + width) + 188).toBeLessThanOrEqual(width - 8);
    },
  );
  it("flips below when there is no room above", () => {
    expect(
      selectionPopoverPosition(
        { left: 100, top: 10, bottom: 31 },
        { left: 0, top: 0, width: 300 },
        { width: 172, height: 29 },
      ).top,
    ).toBe(37);
  });
  it("projects offsets and rejects user or cross-answer selections", () => {
    document.body.innerHTML =
      '<div id="content"><div data-selectable-answer="turn-1" data-item-id="item-1"><p>Hello <strong>world</strong></p></div><div data-selectable-answer="turn-2" data-item-id="item-2">Second</div><p id="user">User</p></div>';
    const content = document.querySelector<HTMLElement>("#content")!;
    const range = document.createRange();
    range.selectNodeContents(content.querySelector("strong")!);
    const selection = window.getSelection()!;
    selection.removeAllRanges();
    selection.addRange(range);
    expect(readTranscriptQuote(selection, content)?.quote).toEqual({
      turnId: "turn-1",
      itemId: "item-1",
      text: "world",
      startOffset: 6,
      endOffset: 11,
    });
    range.setEnd(
      content.querySelector('[data-item-id="item-2"]')!.firstChild!,
      3,
    );
    expect(readTranscriptQuote(selection, content)).toBeNull();
    range.selectNodeContents(content.querySelector("#user")!);
    expect(readTranscriptQuote(selection, content)).toBeNull();
    selection.removeAllRanges();
  });
});
