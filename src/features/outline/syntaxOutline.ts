import { language as languageFacet } from "@codemirror/language";
import { EditorState } from "@codemirror/state";
import {
  TreeFragment,
  type Parser,
  type SyntaxNode,
  type Tree,
} from "@lezer/common";

import {
  editorLanguageForPath,
  loadEditorLanguage,
  type EditorLanguage,
} from "../files/editorLanguage";
import {
  MAX_OUTLINE_SOURCE_CHARS,
  outlineAvailability,
} from "./outlineAvailability";
import {
  collectorFor,
  type Collect,
  type OutlineSink,
} from "./outlineCollectors";

export { MAX_OUTLINE_SOURCE_CHARS };

/** Matches the WebView list caps (Explorer 500, Search 500). */
export const MAX_OUTLINE_SYMBOLS = 500;
const MAX_SYMBOL_NAME_CHARS = 160;
export const MAX_OUTLINE_DEPTH = 8;
export const OUTLINE_PARSE_SLICE_MS = 6;

export type OutlineSymbolKind =
  | "function"
  | "class"
  | "type"
  | "variable"
  | "module"
  | "heading"
  | "property"
  | "rule";

export interface OutlineSymbol {
  name: string;
  kind: OutlineSymbolKind;
  /** 1-based line and code-point column of the declaration name. */
  line: number;
  column: number;
  /** UTF-16 offset of the declaration name, for caret navigation. */
  offset: number;
  /** Zero-based; a symbol's descendants follow it contiguously. */
  depth: number;
}

export interface SyntaxOutline {
  symbols: readonly OutlineSymbol[];
  truncated: boolean;
}

export async function syntaxOutline(
  path: string,
  source: string,
  current: () => boolean = () => true,
): Promise<SyntaxOutline | null> {
  if (outlineAvailability(path, source.length) !== "available") return null;
  const language = editorLanguageForPath(path);
  const collect = collectorFor(language);
  if (!collect) return null;
  const extension = await loadEditorLanguage(language);
  if (!current()) return null;
  const parser = EditorState.create({ extensions: extension }).facet(
    languageFacet,
  )?.parser;
  if (!parser) return null;
  const tree = await slicedParse(parser, language, source, current);
  return tree ? outlineFromTree(tree, source, collect, current) : null;
}

/** The last complete parse; one slot bounds memory to one File's tree. */
let reusable: { language: EditorLanguage; source: string; tree: Tree } | null =
  null;

async function slicedParse(
  parser: Parser,
  language: EditorLanguage,
  source: string,
  current: () => boolean,
): Promise<Tree | null> {
  const previous = reusable?.language === language ? reusable : null;
  if (previous?.source === source) return previous.tree;
  // The text diff counts against the first slice, not on top of it.
  let sliceEnd = performance.now() + OUTLINE_PARSE_SLICE_MS;
  const fragments = previous
    ? TreeFragment.applyChanges(TreeFragment.addTree(previous.tree), [
        changedRange(previous.source, source),
      ])
    : undefined;
  const parse = parser.startParse(source, fragments);
  for (;;) {
    let tree: Tree | null = null;
    while (!tree && performance.now() < sliceEnd) tree = parse.advance();
    if (tree) {
      reusable = { language, source, tree };
      return tree;
    }
    await yieldToMainThread();
    if (!current()) return null;
    sliceEnd = performance.now() + OUTLINE_PARSE_SLICE_MS;
  }
}

function changedRange(before: string, after: string) {
  const shorter = Math.min(before.length, after.length);
  let start = 0;
  while (
    start < shorter &&
    before.charCodeAt(start) === after.charCodeAt(start)
  ) {
    start += 1;
  }
  let end = 0;
  while (
    end < shorter - start &&
    before.charCodeAt(before.length - 1 - end) ===
      after.charCodeAt(after.length - 1 - end)
  ) {
    end += 1;
  }
  return {
    fromA: start,
    toA: before.length - end,
    fromB: start,
    toB: after.length - end,
  };
}

function yieldToMainThread(): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, 0));
}

interface OutlineFrame {
  node: SyntaxNode | null;
  depth: number;
  collect: Collect;
}

async function outlineFromTree(
  tree: Tree,
  source: string,
  collect: Collect,
  current: () => boolean,
): Promise<SyntaxOutline | null> {
  const symbols: OutlineSymbol[] = [];
  const position = positionCounter(source);
  let truncated = false;
  let depth = 0;
  const headingLevels: number[] = [];
  const push = (name: string, kind: OutlineSymbolKind, at: number) => {
    if (symbols.length >= MAX_OUTLINE_SYMBOLS) {
      truncated = true;
      return false;
    }
    const text = symbolName(name);
    if (!text) return false;
    symbols.push({ name: text, kind, ...position(at), offset: at, depth });
    return true;
  };
  const sink: OutlineSink = {
    text: (from, to) => source.slice(from, to),
    emit: push,
    emitHeading(name, level, at) {
      while ((headingLevels.at(-1) ?? 0) >= level) headingLevels.pop();
      // Six heading levels nest at most five deep, inside MAX_OUTLINE_DEPTH.
      depth = headingLevels.length;
      if (push(name, "heading", at)) headingLevels.push(level);
    },
    members(container, collectMembers) {
      if (!container || depth + 1 >= MAX_OUTLINE_DEPTH) return;
      queued.push({
        node: container.firstChild,
        depth: depth + 1,
        collect: collectMembers,
      });
    },
    full: () => truncated,
  };
  // One explicit stack walks top-level and nested declarations alike, so
  // every node counts against the same slice budget. Members queued while a
  // node is collected run next, in order, which keeps document order.
  const frames: OutlineFrame[] = [
    { node: tree.topNode.firstChild, depth: 0, collect },
  ];
  const queued: OutlineFrame[] = [];
  let sliceEnd = performance.now() + OUTLINE_PARSE_SLICE_MS;
  while (frames.length && !truncated) {
    const frame = frames[frames.length - 1]!;
    const node = frame.node;
    if (!node) {
      frames.pop();
      continue;
    }
    frame.node = node.nextSibling;
    depth = frame.depth;
    frame.collect(node, sink);
    while (queued.length) frames.push(queued.pop()!);
    if (performance.now() >= sliceEnd) {
      await yieldToMainThread();
      if (!current()) return null;
      sliceEnd = performance.now() + OUTLINE_PARSE_SLICE_MS;
    }
  }
  return { symbols, truncated };
}

function symbolName(raw: string): string {
  const name = raw.replace(/\s+/gu, " ").trim();
  return name.length > MAX_SYMBOL_NAME_CHARS
    ? `${name.slice(0, MAX_SYMBOL_NAME_CHARS - 1)}…`
    : name;
}

/** Monotonic lookup in document order; code-point columns match File navigation, including emoji. */
function positionCounter(
  source: string,
): (offset: number) => { line: number; column: number } {
  let line = 1;
  let column = 1;
  let scanned = 0;
  return (offset) => {
    while (scanned < offset) {
      const code = source.charCodeAt(scanned);
      scanned += 1;
      if (code === 10) {
        line += 1;
        column = 1;
      } else if (code < 0xdc00 || code > 0xdfff) {
        // A low surrogate completes a pair already counted by its high half.
        column += 1;
      }
    }
    return { line, column };
  };
}
