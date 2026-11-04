import { describe, expect, it } from "vitest";
import fixture from "./fixtures/commonmark-0.31.2-blocks.json";
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

// Inline text, formatting and tight/loose paragraph wrappers are immaterial
// here. Container ownership, list boundaries, starts and heading levels are not.
function structure(block: MarkdownPreviewBlock): Structure {
  if (block.kind === "heading") return { kind: block.kind, level: block.level };
  if (block.kind === "quote")
    return {
      kind: block.kind,
      children: block.text ? [{ kind: "paragraph" }] : [],
    };
  if (block.kind !== "list") return { kind: block.kind };
  const listBlock = block;
  const shape = (at: number) =>
    listBlock.shape?.[at] ?? { depth: 0, ordered: !!listBlock.ordered };
  let cursor = 0;
  function list(): Structure {
    const { depth, ordered } = shape(cursor);
    const result: Structure = {
      kind: "list",
      ordered,
      ...(ordered
        ? { start: depth ? (shape(cursor).start ?? 1) : (listBlock.start ?? 1) }
        : {}),
      items: [],
    };
    const from = cursor;
    while (cursor < listBlock.items.length && shape(cursor).depth >= depth) {
      if (shape(cursor).depth > depth) result.items!.at(-1)!.push(list());
      else {
        if (
          cursor > from &&
          (shape(cursor).ordered !== ordered || shape(cursor).newList)
        )
          break;
        result.items!.push(
          listBlock.items[cursor] ? [{ kind: "paragraph" }] : [],
        );
        cursor++;
      }
    }
    return result;
  }
  return list();
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
