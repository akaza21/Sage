import { readFileSync } from "node:fs";
import { resolve } from "node:path";

import { cleanup, render } from "@testing-library/react";
import { afterEach, beforeAll, describe, expect, it } from "vitest";

import { MarkdownBlocks } from "../../files/MarkdownBlocks";
import { renderMarkdownPreview } from "../../files/markdownPreview";
import { ChatMarkdown } from "./ChatMarkdown";
import nextStreamRuns from "./streamTail";
import { loadStreamRuns, streamRunTracker } from "./ChatMarkdown";

// The run tracker is a lazy chunk that starts loading with ChatMarkdown.
beforeAll(() => loadStreamRuns());
afterEach(cleanup);

it("loads the run tracker as its own chunk, off the first-Chat path", async () => {
  await loadStreamRuns();
  expect(streamRunTracker()).toBeTypeOf("function");
  const markdown = readFileSync(
    resolve("src/features/chat/acp/ChatMarkdown.tsx"),
    "utf8",
  );
  expect(markdown).toMatch(/import\("\.\/streamTail"\)/);
  expect(markdown).not.toMatch(/import \{[^}]*\} from "\.\/streamTail"/);
});

const doc = renderMarkdownPreview;
const newestRuns = (
  previous: ReturnType<typeof doc> | null,
  next: ReturnType<typeof doc>,
) => nextStreamRuns(null, previous, next);
const tails = (container: HTMLElement) =>
  [...container.querySelectorAll(".acp-stream-new")].map(
    (node) => node.textContent,
  );

it.each([
  ["plain", "plain text", [100]],
  ["Hi **bold** tail", "Hi **bold** tail more", [5]],
])(
  "resets stale runs at the rendered inline boundary: %s",
  (before, after, runs) => {
    expect(
      nextStreamRuns(
        { blockIndex: 0, itemIndex: undefined, runs },
        doc(before),
        doc(after),
      )?.runs,
    ).toEqual([5]);
  },
);

it("resets oversized rendered runs rather than adding empty or duplicated spans", () => {
  const { container } = render(
    <MarkdownBlocks
      document={doc("hello world")}
      streamRuns={{ blockIndex: 0, itemIndex: undefined, runs: [20, 3] }}
    />,
  );
  expect(tails(container)).toEqual(["rld"]);
  expect(container.querySelector("p")).toHaveTextContent("hello world");
});

it("adjusts every surrogate boundary against the original offsets", () => {
  const { container } = render(
    <MarkdownBlocks
      document={doc("Hi 😀😀x")}
      streamRuns={{ blockIndex: 0, itemIndex: undefined, runs: [1, 1, 1] }}
    />,
  );
  expect(tails(container)).toEqual(["😀", "x"]);
  expect(container.querySelector("p")).toHaveTextContent("Hi 😀😀x");
});

it.each(["**bold** tail more", "[file](src/main.ts) tail more"])(
  "resets runs that no longer cover the whole formatted tail: %s",
  (text) => {
    const { container } = render(
      <MarkdownBlocks
        document={doc(text)}
        streamRuns={{ blockIndex: 0, itemIndex: undefined, runs: [5, 4] }}
      />,
    );
    expect(tails(container)).toEqual(["more"]);
    expect(container.querySelectorAll(".acp-stream-new")).toHaveLength(1);
  },
);

describe("nextStreamRuns target", () => {
  it("resets empty, shortened, retyped and structurally new targets", () => {
    expect(newestRuns(doc(""), doc(""))).toBeNull();
    expect(newestRuns(doc("long text"), doc("long"))).toBeNull();
    expect(newestRuns(doc("title"), doc("# title more"))).toBeNull();
    expect(
      newestRuns(doc("| a |\n| - |"), doc("| a |\n| - |\n| b |")),
    ).toBeNull();
    expect(
      newestRuns(doc("| a |\n| - |\n| b |"), doc("| a |\n| - |\n| b |\n| c |")),
    ).toBeNull();
  });

  it("targets the growing last paragraph by the appended length", () => {
    expect(newestRuns(doc("Top hit:"), doc("Top hit: Streaming API"))).toEqual({
      blockIndex: 0,
      itemIndex: undefined,
      runs: [14],
    });
  });

  it("targets the last item of a growing list and the last cell of a growing table row", () => {
    expect(
      newestRuns(doc("- one\n- tw"), doc("- one\n- two words")),
    ).toMatchObject({ blockIndex: 0, itemIndex: 1, runs: [7] });
    expect(
      newestRuns(
        doc("| a | b |\n| - | - |\n| 1 | tw |"),
        doc("| a | b |\n| - | - |\n| 1 | two |"),
      ),
    ).toMatchObject({ blockIndex: 0, itemIndex: 3, runs: [1] });
  });

  it("measures the run in rendered text, not raw markdown", () => {
    // Collapsed whitespace and stripped markers are not rendered characters.
    expect(
      newestRuns(doc("Top hit:"), doc("Top hit:   lots\n  more")),
    ).toMatchObject({ runs: [10] });
    expect(
      newestRuns(doc("Say **hi**"), doc("Say **hi** there **you**")),
    ).toMatchObject({ runs: [10] });
    // A chunk that re-renders earlier text (a marker closing) has no run.
    expect(newestRuns(doc("Say **hi"), doc("Say **hi** there"))).toBeNull();
  });

  it("leaves a new block, item or row to its own whole-element fade", () => {
    expect(newestRuns(doc("One."), doc("One.\n\nTwo"))).toBeNull();
    expect(newestRuns(doc("- one"), doc("- one\n- two"))).toBeNull();
    expect(newestRuns(null, doc("One."))).toBeNull();
    expect(newestRuns(doc("One."), doc("One."))).toBeNull();
    expect(newestRuns(doc("```js\nlet"), doc("```js\nlet a"))).toBeNull();
  });
});

describe("ChatMarkdown appended-text fade (Codex s5/f0309)", () => {
  it("marks only appended runs of a growing paragraph and keeps each through the next chunks (f0309)", () => {
    const { container, rerender } = render(
      <ChatMarkdown text="Top hit:" streaming />,
    );
    expect(tails(container)).toEqual([]);
    rerender(<ChatMarkdown text="Top hit: Streaming API" streaming />);
    expect(tails(container)).toEqual([" Streaming API"]);
    expect(container.querySelector("p")).toHaveTextContent(
      "Top hit: Streaming API",
    );
    const first = container.querySelector(".acp-stream-new")!;
    rerender(
      <ChatMarkdown
        text="Top hit: Streaming API responses explain"
        streaming
      />,
    );
    // The next chunk does not cut the earlier run's fade short: the same
    // node keeps its class, so its animation runs its full 300 ms.
    expect(tails(container)).toEqual([" Streaming API", " responses explain"]);
    expect(container.querySelector(".acp-stream-new")).toBe(first);
    expect(container.querySelector("p")?.firstChild?.textContent).toBe(
      "Top hit:",
    );
    expect(container.querySelector("p")).toHaveTextContent(
      "Top hit: Streaming API responses explain",
    );
  });

  it("starts afresh in a new block, which fades whole", () => {
    const { container, rerender } = render(<ChatMarkdown text="A" streaming />);
    rerender(<ChatMarkdown text="A b" streaming />);
    rerender(<ChatMarkdown text="A b c" streaming />);
    expect(tails(container)).toEqual([" b", " c"]);
    rerender(<ChatMarkdown text={"A b c\n\nD"} streaming />);
    expect(tails(container)).toEqual([]);
    rerender(<ChatMarkdown text={"A b c\n\nD e"} streaming />);
    expect(tails(container)).toEqual([" e"]);
  });

  it("drops a run whose text re-rendered and bounds the runs that fade at once", () => {
    const { container, rerender } = render(
      <ChatMarkdown text="Say **hi" streaming />,
    );
    rerender(<ChatMarkdown text="Say **hi there" streaming />);
    expect(tails(container)).toEqual([" there"]);
    // Closing the marker re-renders the earlier text: the run goes.
    rerender(<ChatMarkdown text="Say **hi there**" streaming />);
    expect(tails(container)).toEqual([]);
    let text = "Go";
    rerender(<ChatMarkdown text={text} streaming />);
    for (let chunk = 0; chunk < 20; chunk++) {
      text += ` w${chunk}`;
      rerender(<ChatMarkdown text={text} streaming />);
    }
    expect(tails(container)).toHaveLength(16);
    expect(tails(container).at(-1)).toBe(" w19");
    expect(container.querySelector("p")).toHaveTextContent(text);
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
        streamRuns={{
          blockIndex: 0,
          itemIndex: undefined,
          runs: [2],
        }}
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

it("does not track a textless rule while streaming its marker", () => {
  const previous = doc("Intro\n\n---");
  const next = doc("Intro\n\n----");
  expect(next.blocks.at(-1)).toEqual({ kind: "rule" });
  expect(newestRuns(previous, next)).toBeNull();
  const { container } = render(
    <ChatMarkdown text={"Intro\n\n---"} streaming />,
  );
  expect(container.querySelector("hr")).not.toBeNull();
});

it.each(["~~~", "```"])(
  "keeps incomplete and over-indented %s closers literal while streaming",
  (fence) => {
    const { container, rerender } = render(
      <ChatMarkdown text={`${fence}js\ncode\n    ~`} streaming />,
    );
    const sources = [
      `${fence}js\ncode\n    ${fence}\nAfter`,
      `${fence}js\ncode\n    ${fence}\nAfter\n  ${fence.slice(0, 2)}`,
    ];
    for (const source of sources) {
      rerender(<ChatMarkdown text={source} streaming />);
      const expected = source.slice(source.indexOf("\n") + 1);
      expect(doc(source).blocks).toEqual([
        { kind: "code", language: "js", text: expected, fenceClosed: false },
      ]);
      expect(
        [...container.querySelectorAll("pre code")].map(
          (node) => node.textContent,
        ),
      ).toEqual([expected]);
      expect([...container.querySelectorAll(".acp-markdown > p")]).toEqual([]);
      expect(tails(container)).toEqual([]);
    }
    const source = `${fence}js\ncode\n    ${fence}\nAfter\n  ${fence}\nDone`;
    rerender(<ChatMarkdown text={source} streaming />);
    expect(doc(source).blocks).toEqual([
      {
        kind: "code",
        language: "js",
        text: `code\n    ${fence}\nAfter`,
        fenceClosed: true,
      },
      { kind: "paragraph", text: "Done" },
    ]);
    expect(
      [...container.querySelectorAll("pre code")].map(
        (node) => node.textContent,
      ),
    ).toEqual([`code\n    ${fence}\nAfter`]);
    expect(
      [...container.querySelectorAll(".acp-markdown > p")].map(
        (node) => node.textContent,
      ),
    ).toEqual(["Done"]);
  },
);

it("reclassifies streaming setext text while keeping an empty list separate from a rule", () => {
  const { container, rerender } = render(
    <ChatMarkdown text="Title" streaming />,
  );
  const surface = () => container.querySelector(".acp-markdown")!.innerHTML;
  expect(surface()).toBe("<p>Title</p>");
  for (const [source, expected] of [
    ["Title\n=", "<h1>Title</h1>"],
    ["Title\n===", "<h1>Title</h1>"],
    ["Title\n-", "<h2>Title</h2>"],
    ["Title\n---", "<h2>Title</h2>"],
    ["-", "<ul><li></li></ul>"],
    ["-\n-", "<ul><li></li><li></li></ul>"],
    ["-\n---", "<ul><li></li></ul><hr>"],
  ]) {
    rerender(<ChatMarkdown text={source} streaming />);
    expect(surface()).toBe(expected);
    expect(tails(container)).toEqual([]);
  }
});
