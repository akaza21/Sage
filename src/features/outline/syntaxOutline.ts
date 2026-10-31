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
import { MAX_OUTLINE_SOURCE_CHARS, outlineAvailability } from "./outlineCard";

export { MAX_OUTLINE_SOURCE_CHARS };

/** Matches the WebView list caps (Explorer 500, Search 500). */
export const MAX_OUTLINE_SYMBOLS = 500;
const MAX_SYMBOL_NAME_CHARS = 160;
/** Longest uninterrupted parse slice; the main thread yields between slices. */
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
  /** 1-based line and UTF-16 column of the declaration name. */
  line: number;
  column: number;
  /** UTF-16 offset of the declaration name, for caret navigation. */
  offset: number;
}

export interface SyntaxOutline {
  symbols: readonly OutlineSymbol[];
  /** True when the document declares more than MAX_OUTLINE_SYMBOLS symbols. */
  truncated: boolean;
}

interface OutlineSink {
  /** Source text of a range, whitespace-collapsed by the sink. */
  text(from: number, to: number): string;
  emit(name: string, kind: OutlineSymbolKind, at: number): void;
  /** True once the cap is reached and further symbols only mark truncation. */
  full(): boolean;
}

type Collect = (node: SyntaxNode, sink: OutlineSink) => void;

/** Outline uses the editor's Lezer grammar. Return null for unsupported languages, oversized Files or stale requests. Yield between slices; reuse unchanged trees. */
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

/** The single edited span between two texts, from their common ends. */
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

async function outlineFromTree(
  tree: Tree,
  source: string,
  collect: Collect,
  current: () => boolean,
): Promise<SyntaxOutline | null> {
  const symbols: OutlineSymbol[] = [];
  const position = positionCounter(source);
  let truncated = false;
  const sink: OutlineSink = {
    text: (from, to) => source.slice(from, to),
    emit(name, kind, at) {
      if (symbols.length >= MAX_OUTLINE_SYMBOLS) {
        truncated = true;
        return;
      }
      const text = symbolName(name);
      if (text) symbols.push({ name: text, kind, ...position(at), offset: at });
    },
    full: () => truncated,
  };
  let sliceEnd = performance.now() + OUTLINE_PARSE_SLICE_MS;
  for (
    let node = tree.topNode.firstChild;
    node && !truncated;
    node = node.nextSibling
  ) {
    collect(node, sink);
    if (performance.now() >= sliceEnd) {
      await yieldToMainThread();
      if (!current()) return null;
      sliceEnd = performance.now() + OUTLINE_PARSE_SLICE_MS;
    }
  }
  return { symbols, truncated };
}

function collectorFor(language: EditorLanguage): Collect | null {
  switch (language) {
    case "javascript":
    case "javascript-jsx":
    case "typescript":
    case "typescript-jsx":
      return collectJavaScript;
    case "python":
      return collectPython;
    case "rust":
      return collectRust;
    case "markdown":
      return collectMarkdown;
    case "json":
      return collectJson;
    case "css":
      return collectCss;
    case "html":
    case "plain":
      return null;
  }
}

function emitNamed(
  node: SyntaxNode,
  nameType: string,
  kind: OutlineSymbolKind,
  sink: OutlineSink,
  fallback?: string,
): void {
  const name = node.getChild(nameType);
  if (name) sink.emit(sink.text(name.from, name.to), kind, name.from);
  else if (fallback) sink.emit(fallback, kind, node.from);
}

const collectJavaScript: Collect = (node, sink) => {
  switch (node.name) {
    case "ExportDeclaration":
    case "AmbientDeclaration":
      for (let child = node.firstChild; child; child = child.nextSibling) {
        collectJavaScript(child, sink);
      }
      return;
    case "FunctionDeclaration":
      return emitNamed(node, "VariableDefinition", "function", sink, "default");
    case "ClassDeclaration":
      return emitNamed(node, "VariableDefinition", "class", sink, "default");
    case "InterfaceDeclaration":
    case "TypeAliasDeclaration":
    case "EnumDeclaration":
      return emitNamed(node, "TypeDefinition", "type", sink);
    case "NamespaceDeclaration":
      return emitNamed(node, "VariableDefinition", "module", sink);
    case "VariableDeclaration":
      for (const name of node.getChildren("VariableDefinition")) {
        sink.emit(sink.text(name.from, name.to), "variable", name.from);
      }
      return;
  }
};

const collectPython: Collect = (node, sink) => {
  switch (node.name) {
    case "DecoratedStatement":
      for (let child = node.firstChild; child; child = child.nextSibling) {
        collectPython(child, sink);
      }
      return;
    case "FunctionDefinition":
      return emitNamed(node, "VariableName", "function", sink);
    case "ClassDefinition":
      return emitNamed(node, "VariableName", "class", sink);
  }
};

const collectRust: Collect = (node, sink) => {
  switch (node.name) {
    case "FunctionItem":
    case "MacroDefinition":
      return emitNamed(
        node,
        node.name === "FunctionItem" ? "BoundIdentifier" : "Identifier",
        "function",
        sink,
      );
    case "StructItem":
    case "EnumItem":
    case "UnionItem":
    case "TraitItem":
    case "TypeItem":
      return emitNamed(node, "TypeIdentifier", "type", sink);
    case "ImplItem": {
      const body = node.getChild("DeclarationList");
      return sink.emit(
        sink.text(node.from, body?.from ?? node.to),
        "type",
        node.from,
      );
    }
    case "ConstItem":
    case "StaticItem":
      return emitNamed(node, "BoundIdentifier", "variable", sink);
    case "ModItem":
      return emitNamed(node, "BoundIdentifier", "module", sink);
  }
};

const collectMarkdown: Collect = (node, sink) => {
  if (!/^(?:ATX|Setext)Heading[1-6]$/u.test(node.name)) return;
  let from = node.from;
  let to = node.to;
  for (const mark of node.getChildren("HeaderMark")) {
    if (mark.from === from) from = mark.to;
    else if (mark.to === to || mark.from > from) to = Math.min(to, mark.from);
  }
  sink.emit(sink.text(from, to).replace(/#+\s*$/u, ""), "heading", node.from);
};

const collectJson: Collect = (node, sink) => {
  if (node.name !== "Object") return;
  // Walk lazily to the cap; materialising large objects would overrun a slice.
  for (
    let property = node.firstChild;
    property && !sink.full();
    property = property.nextSibling
  ) {
    if (property.name !== "Property") continue;
    const name = property.getChild("PropertyName");
    if (name) sink.emit(sink.text(name.from, name.to), "property", name.from);
  }
};

const collectCss: Collect = (node, sink) => {
  if (node.name !== "RuleSet" && !node.name.endsWith("Statement")) return;
  const block = node.getChild("Block");
  const end = block?.from ?? node.to;
  sink.emit(sink.text(node.from, end).replace(/;$/u, ""), "rule", node.from);
};

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
