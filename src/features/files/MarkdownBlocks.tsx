import { createElement, Fragment, type ReactNode } from "react";
import type {
  InlineFormat,
  MarkdownPreviewBlock,
  MarkdownPreviewDocument,
} from "./markdownPreview";
import { MermaidFigureBoundary } from "../chat/acp/MermaidFigureBoundary";

export function MarkdownBlocks({
  document,
  renderLink = (index) => document.fileLinks[index].label,
  renderCode,
}: {
  document: MarkdownPreviewDocument;
  renderLink?: (index: number) => ReactNode;
  renderCode?: (
    block: Extract<MarkdownPreviewBlock, { kind: "code" }>,
  ) => ReactNode;
}) {
  const inline = (text: string, blockIndex: number, itemIndex?: number) => {
    const spans =
      document.formats.find(
        (format) =>
          format.blockIndex === blockIndex && format.itemIndex === itemIndex,
      )?.spans ?? [];
    const render = (formats: InlineFormat[]): ReactNode[] =>
      formats.map((format, index) =>
        typeof format === "string"
          ? format
          : "link" in format
            ? createElement(Fragment, { key: index }, renderLink(format.link))
            : createElement(
                format.tag,
                { key: index },
                render(format.children),
              ),
      );
    return spans.length ? render(spans) : text;
  };
  return document.blocks.map((block, index) => {
    if (block.kind === "code")
      return (
        <div key={index}>
          {renderCode ? (
            renderCode(block)
          ) : block.language.toLowerCase() === "mermaid" ? (
            <MermaidFigureBoundary source={block.text} />
          ) : (
            <pre>{block.text}</pre>
          )}
        </div>
      );
    if (
      block.kind === "heading" ||
      block.kind === "paragraph" ||
      block.kind === "quote"
    )
      return createElement(
        block.kind === "heading"
          ? `h${block.level}`
          : block.kind === "quote"
            ? "blockquote"
            : "p",
        { key: index },
        inline(block.text, index),
      );
    if (block.kind === "list")
      return createElement(
        block.ordered ? "ol" : "ul",
        { key: index, start: block.start },
        block.items.map((item, itemIndex) => (
          <li key={itemIndex}>{inline(item, index, itemIndex)}</li>
        )),
      );
    const row = (cells: string[], rowIndex: number) => (
      <tr key={rowIndex}>
        {cells.map((cell, column) =>
          createElement(
            rowIndex ? "td" : "th",
            {
              key: column,
              scope: rowIndex ? undefined : "col",
              style: { textAlign: block.align[column] },
            },
            inline(cell, index, rowIndex * block.align.length + column),
          ),
        )}
      </tr>
    );
    return (
      <div
        className="markdown-table"
        key={index}
        tabIndex={0}
        role="region"
        aria-label="Table"
      >
        <table>
          <thead>{row(block.rows[0], 0)}</thead>
          <tbody>
            {block.rows
              .slice(1)
              .map((cells, rowIndex) => row(cells, rowIndex + 1))}
          </tbody>
        </table>
      </div>
    );
  });
}
