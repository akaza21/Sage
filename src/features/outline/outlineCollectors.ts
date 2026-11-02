/** Per-language walks from a Lezer syntax tree to Outline symbols. */
import type { SyntaxNode } from "@lezer/common";

import type { EditorLanguage } from "../files/editorLanguage";
import type { OutlineSymbolKind } from "./syntaxOutline";

export interface OutlineSink {
  /** Source text of a range, whitespace-collapsed by the sink. */
  text(from: number, to: number): string;
  /** Records a symbol at the current depth; false when it was not kept. */
  emit(name: string, kind: OutlineSymbolKind, at: number): boolean;
  /**
   * Records a heading, nesting it under the closest preceding heading of a
   * higher level (a smaller number).
   */
  emitHeading(name: string, level: number, at: number): void;
  /** Collects each child of `container` one level below the current depth. */
  members(container: SyntaxNode | null, collect: Collect): void;
  /** True once the cap is reached and further symbols only mark truncation. */
  full(): boolean;
}

export type Collect = (node: SyntaxNode, sink: OutlineSink) => void;

export function collectorFor(language: EditorLanguage): Collect | null {
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
): boolean {
  const name = node.getChild(nameType);
  if (name) return sink.emit(sink.text(name.from, name.to), kind, name.from);
  return fallback ? sink.emit(fallback, kind, node.from) : false;
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
      emitNamed(node, "VariableDefinition", "function", sink, "default");
      return;
    case "ClassDeclaration":
      if (emitNamed(node, "VariableDefinition", "class", sink, "default")) {
        sink.members(node.getChild("ClassBody"), collectJavaScriptMember);
      }
      return;
    case "InterfaceDeclaration":
      if (emitNamed(node, "TypeDefinition", "type", sink)) {
        sink.members(node.getChild("ObjectType"), collectJavaScriptMember);
      }
      return;
    case "TypeAliasDeclaration":
    case "EnumDeclaration":
      emitNamed(node, "TypeDefinition", "type", sink);
      return;
    case "NamespaceDeclaration":
      if (emitNamed(node, "VariableDefinition", "module", sink)) {
        sink.members(node.getChild("Block"), collectJavaScript);
      }
      return;
    case "VariableDeclaration":
      for (const name of node.getChildren("VariableDefinition")) {
        sink.emit(sink.text(name.from, name.to), "variable", name.from);
      }
      return;
  }
};

/** Members of a class body or an interface's object type. */
const collectJavaScriptMember: Collect = (node, sink) => {
  switch (node.name) {
    case "MethodDeclaration":
    case "MethodType":
      emitNamed(node, "PropertyDefinition", "function", sink);
      return;
    case "PropertyDeclaration":
    case "PropertyType":
      emitNamed(node, "PropertyDefinition", "property", sink);
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
      emitNamed(node, "VariableName", "function", sink);
      return;
    case "ClassDefinition":
      if (emitNamed(node, "VariableName", "class", sink)) {
        sink.members(node.getChild("Body"), collectPython);
      }
      return;
  }
};

const collectRust: Collect = (node, sink) => {
  switch (node.name) {
    case "FunctionItem":
    case "MacroDefinition":
      emitNamed(
        node,
        node.name === "FunctionItem" ? "BoundIdentifier" : "Identifier",
        "function",
        sink,
      );
      return;
    case "StructItem":
    case "EnumItem":
    case "UnionItem":
    case "TypeItem":
      emitNamed(node, "TypeIdentifier", "type", sink);
      return;
    case "TraitItem":
      if (emitNamed(node, "TypeIdentifier", "type", sink)) {
        sink.members(node.getChild("DeclarationList"), collectRust);
      }
      return;
    case "ImplItem": {
      const body = node.getChild("DeclarationList");
      const header = sink.text(node.from, body?.from ?? node.to);
      if (sink.emit(header, "type", node.from)) sink.members(body, collectRust);
      return;
    }
    case "ConstItem":
    case "StaticItem":
      emitNamed(node, "BoundIdentifier", "variable", sink);
      return;
    case "ModItem":
      if (emitNamed(node, "BoundIdentifier", "module", sink)) {
        sink.members(node.getChild("DeclarationList"), collectRust);
      }
      return;
  }
};

const collectMarkdown: Collect = (node, sink) => {
  const level = /^(?:ATX|Setext)Heading([1-6])$/u.exec(node.name)?.[1];
  if (!level) return;
  let from = node.from;
  let to = node.to;
  for (const mark of node.getChildren("HeaderMark")) {
    if (mark.from === from) from = mark.to;
    else if (mark.to === to || mark.from > from) to = Math.min(to, mark.from);
  }
  sink.emitHeading(
    sink.text(from, to).replace(/#+\s*$/u, ""),
    Number(level),
    node.from,
  );
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
