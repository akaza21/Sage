import { describe, expect, it } from "vitest";
import fixture from "./fixtures/commonmark-0.31.2-blocks.json";
import tasks from "./fixtures/gfm-0.29-task-lists.json";
import {
  renderMarkdownPreview,
  type MarkdownPreviewBlock,
} from "./markdownPreview";

interface Structure {
  kind: string;
  level?: number;
  ordered?: boolean;
  start?: number;
  children?: Structure[];
  items?: Structure[][];
}

function structure(block: MarkdownPreviewBlock): Structure {
  if (block.kind === "heading") return { kind: block.kind, level: block.level };
  if (block.kind === "quote")
    return { kind: "quote", children: block.children.map(structure) };
  if (block.kind !== "list") return { kind: block.kind };
  return {
    kind: "list",
    ordered: !!block.ordered,
    ...(block.ordered ? { start: block.start ?? 1 } : {}),
    items: block.items.map((item) => item.children.map(structure)),
  };
}

describe("CommonMark 0.31.2 supported block structure", () => {
  it.each(fixture.examples.filter((example) => !example.excluded))(
    "$section example $example",
    ({ markdown, expected }) => {
      const document = renderMarkdownPreview(markdown);
      expect(document.truncated).toBe(false);
      expect(document.blocks.map(structure)).toEqual(expected);
    },
  );
});

describe("GFM 0.29 task list items extension", () => {
  it.each(tasks.examples)("example $example", ({ markdown, expected }) => {
    const document = renderMarkdownPreview(markdown);
    expect(document.truncated).toBe(false);
    expect(document.blocks).toEqual(expected);
  });
});

describe("CommonMark list paragraph interruption sweep", () => {
  const paragraph: Structure = { kind: "paragraph" };
  const emptySiblings: Structure[] = [
    { kind: "list", ordered: false, items: [[paragraph], []] },
  ];
  const contentSiblings: Structure[] = [
    { kind: "list", ordered: false, items: [[paragraph], [paragraph]] },
  ];
  const emptyNested: Structure[] = [
    {
      kind: "list",
      ordered: false,
      items: [[paragraph, { kind: "list", ordered: false, items: [[]] }]],
    },
  ];
  const contentNested: Structure[] = [
    {
      kind: "list",
      ordered: false,
      items: [
        [paragraph, { kind: "list", ordered: false, items: [[paragraph]] }],
      ],
    },
  ];

  // Spec 5.2 exception 1 and examples 281/285/294:
  // https://spec.commonmark.org/0.31.2/#list-items
  const cases = [
    {
      marker: "+",
      indent: "",
      position: "sibling",
      paragraph: emptySiblings,
      blank: emptySiblings,
    },
    {
      marker: "+ two",
      indent: "",
      position: "sibling",
      paragraph: contentSiblings,
      blank: contentSiblings,
    },
    {
      marker: "+",
      indent: "  ",
      position: "nested",
      paragraph: [{ kind: "list", ordered: false, items: [[paragraph]] }],
      blank: emptyNested,
    },
    {
      marker: "+ two",
      indent: "  ",
      position: "nested",
      paragraph: contentNested,
      blank: contentNested,
    },
  ].flatMap(({ marker, indent, position, paragraph, blank }) =>
    (["paragraph", "blank"] as const).map((after) => ({
      marker,
      position,
      after,
      markdown: `+ one\n${after === "blank" ? "\n" : ""}${indent}${marker}\n`,
      expected: after === "paragraph" ? paragraph : blank,
    })),
  );

  it.each(cases)(
    "$position $marker after a $after line",
    ({ markdown, expected }) => {
      const document = renderMarkdownPreview(markdown);
      expect(document.truncated).toBe(false);
      expect(document.blocks.map(structure)).toEqual(expected);
    },
  );
});
