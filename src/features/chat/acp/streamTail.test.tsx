import { readFileSync } from "node:fs";
import { resolve } from "node:path";

import { cleanup, render } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";

import { MarkdownBlocks } from "../../files/MarkdownBlocks";
import { renderMarkdownPreview } from "../../files/markdownPreview";
import { ChatMarkdown } from "./ChatMarkdown";
import { streamTailTarget } from "./streamTail";

afterEach(cleanup);

const doc = renderMarkdownPreview;

describe("streamTailTarget", () => {
  it("targets the growing last paragraph by the appended length", () => {
    expect(
      streamTailTarget(doc("Top hit:"), doc("Top hit: Streaming API"), 22),
    ).toEqual({ blockIndex: 0, itemIndex: undefined, chars: 14, key: 22 });
  });

  it("targets the last item of a growing list and the last cell of a growing table row", () => {
    expect(
      streamTailTarget(doc("- one\n- tw"), doc("- one\n- two words"), 17),
    ).toMatchObject({ blockIndex: 0, itemIndex: 1, chars: 7 });
    expect(
      streamTailTarget(
        doc("| a | b |\n| - | - |\n| 1 | tw |"),
        doc("| a | b |\n| - | - |\n| 1 | two |"),
        31,
      ),
    ).toMatchObject({ blockIndex: 0, itemIndex: 3, chars: 1 });
  });

  it("measures the run in rendered text, not raw markdown", () => {
    // Collapsed whitespace and stripped markers are not rendered characters.
    expect(
      streamTailTarget(doc("Top hit:"), doc("Top hit:   lots\n  more"), 22),
    ).toMatchObject({ chars: 10 });
    expect(
      streamTailTarget(doc("Say **hi**"), doc("Say **hi** there **you**"), 24),
    ).toMatchObject({ chars: 10 });
    // A chunk that re-renders earlier text (a marker closing) has no run.
    expect(
      streamTailTarget(doc("Say **hi"), doc("Say **hi** there"), 16),
    ).toBeNull();
  });

  it("leaves a new block, item or row to its own whole-element fade", () => {
    expect(streamTailTarget(doc("One."), doc("One.\n\nTwo"), 9)).toBeNull();
    expect(streamTailTarget(doc("- one"), doc("- one\n- two"), 11)).toBeNull();
    expect(streamTailTarget(null, doc("One."), 4)).toBeNull();
    expect(streamTailTarget(doc("One."), doc("One."), 4)).toBeNull();
    expect(
      streamTailTarget(doc("```js\nlet"), doc("```js\nlet a"), 11),
    ).toBeNull();
  });
});

describe("ChatMarkdown appended-text fade (Codex s5/f0309)", () => {
  const tails = (container: HTMLElement) =>
    [...container.querySelectorAll(".acp-stream-new")].map(
      (node) => node.textContent,
    );

  it("marks only the newest appended run of a growing paragraph", () => {
    const { container, rerender } = render(
      <ChatMarkdown text="Top hit:" streaming />,
    );
    expect(tails(container)).toEqual([]);
    rerender(<ChatMarkdown text="Top hit: Streaming API" streaming />);
    expect(tails(container)).toEqual([" Streaming API"]);
    expect(container.querySelector("p")).toHaveTextContent(
      "Top hit: Streaming API",
    );
    rerender(
      <ChatMarkdown
        text="Top hit: Streaming API responses explain"
        streaming
      />,
    );
    // The earlier run is now plain, solid text.
    expect(tails(container)).toEqual([" responses explain"]);
    expect(container.querySelector("p")?.firstChild?.textContent).toBe(
      "Top hit: Streaming API",
    );
  });

  it("trails exactly the rendered newest run when the chunk carries collapsed whitespace", () => {
    const { container, rerender } = render(
      <ChatMarkdown text="Top hit:" streaming />,
    );
    rerender(<ChatMarkdown text={"Top hit:   lots\n  more"} streaming />);
    expect(tails(container)).toEqual([" lots more"]);
  });

  it("never splits a surrogate pair", () => {
    const { container } = render(
      <MarkdownBlocks
        document={doc("Hi \u{1F600}x")}
        streamTail={{ blockIndex: 0, itemIndex: undefined, chars: 2, key: 1 }}
      />,
    );
    expect(tails(container)).toEqual(["\u{1F600}x"]);
    expect(container.querySelector("p")).toHaveTextContent("Hi \u{1F600}x");
  });

  it("keeps the newest run marked across a re-render without new text", () => {
    const { container, rerender } = render(
      <ChatMarkdown text="Top hit:" streaming />,
    );
    rerender(<ChatMarkdown text="Top hit: more" streaming />);
    const tail = container.querySelector(".acp-stream-new");
    rerender(<ChatMarkdown text="Top hit: more" streaming />);
    expect(container.querySelector(".acp-stream-new")).toBe(tail);
  });

  it("marks nothing for a new block or once the answer settles", () => {
    const { container, rerender } = render(
      <ChatMarkdown text="One." streaming />,
    );
    rerender(<ChatMarkdown text={"One.\n\nTwo"} streaming />);
    expect(tails(container)).toEqual([]);
    rerender(<ChatMarkdown text={"One.\n\nTwo words"} streaming />);
    expect(tails(container)).toEqual([" words"]);
    rerender(<ChatMarkdown text={"One.\n\nTwo words"} />);
    expect(tails(container)).toEqual([]);
    expect(container.querySelectorAll("p")[1]).toHaveTextContent("Two words");
  });

  it("fades the marked run on the stream curve, except under reduced motion", () => {
    const sheet = readFileSync(
      resolve("src/features/chat/acp/chat-markdown.css"),
      "utf8",
    );
    expect(sheet).toMatch(
      /\[data-turn-state\^="waiting"\]\)\s*\.acp-message--assistant\s*\.acp-stream-new\s*\{\s*animation:\s*acp-stream-in var\(--motion-duration-stream\) ease-out both;/,
    );
    expect(sheet).toMatch(
      /@media \(prefers-reduced-motion: reduce\)\s*\{[^@]*\.acp-stream-new[^}]*\{\s*animation:\s*none;/,
    );
  });
});
