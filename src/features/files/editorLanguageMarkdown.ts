import { autoCloseTags, htmlLanguage } from "@codemirror/lang-html";
import { commonmarkLanguage, pasteURLAsLink } from "@codemirror/lang-markdown";
import {
  Language,
  LanguageSupport,
  foldService,
  syntaxTree,
} from "@codemirror/language";
import type { SyntaxNode } from "@lezer/common";
import { parseCode, type MarkdownParser } from "@lezer/markdown";

const htmlNoMatch = htmlLanguage.configure({ dialect: "noMatch" });

function headingLevel(name: string): number | null {
  const match = /^(?:ATX|Setext)Heading(\d)$/.exec(name);
  return match ? Number(match[1]) : null;
}

const headerIndent = foldService.of((state, start, end) => {
  let node: SyntaxNode | null = syntaxTree(state).resolveInner(end, -1);
  while (node) {
    if (node.from < start) {
      break;
    }
    const level = headingLevel(node.name);
    if (level != null) {
      let last = node;
      for (;;) {
        const next = last.nextSibling;
        if (!next) {
          break;
        }
        const nextLevel = headingLevel(next.name);
        if (nextLevel != null && nextLevel <= level) {
          break;
        }
        last = next;
      }
      if (last.to > end) {
        return { from: end, to: last.to };
      }
    }
    node = node.parent;
  }
  return null;
});

const markdownLanguage = new Language(
  commonmarkLanguage.data,
  (commonmarkLanguage.parser as MarkdownParser).configure(
    parseCode({ htmlParser: htmlNoMatch.parser }),
  ),
  [],
  "markdown",
);

export const loadMarkdownEditorLanguage = () =>
  new LanguageSupport(markdownLanguage, [
    [htmlNoMatch, autoCloseTags],
    headerIndent,
    pasteURLAsLink,
  ]);
