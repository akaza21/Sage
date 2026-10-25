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
  streamTail,
}: {
  document: MarkdownPreviewDocument;
  renderLink?: (index: number) => ReactNode;
  renderCode?: (
    block: Extract<MarkdownPreviewBlock, { kind: "code" }>,
  ) => ReactNode;
  /** A streamed answer's newest run, wrapped so only it fades. */
  streamTail?: {
    blockIndex: number;
    itemIndex: number | undefined;
    chars: number;
    key: number;
  } | null;
}) {
  const formatsByLocation = new Map(
    document.formats.map((format) => [
      `${format.blockIndex}:${format.itemIndex}`,
      format.spans,
    ]),
  );
  const inline = (text: string, blockIndex: number, itemIndex?: number) => {
    const content = formatted(text, blockIndex, itemIndex);
    if (
      streamTail?.blockIndex !== blockIndex ||
      streamTail.itemIndex !== itemIndex
    )
      return content;
    // Only a trailing plain-text run is split; formatted ends stay whole.
    const nodes = Array.isArray(content) ? [...content] : [content];
    const last = nodes.at(-1);
    if (typeof last !== "string" || !last) return content;
    let split = Math.max(0, last.length - streamTail.chars);
    // Never split a surrogate pair: its whole character joins the run.
    if (split > 0 && /[\uDC00-\uDFFF]/.test(last[split])) split -= 1;
    nodes[nodes.length - 1] = last.slice(0, split);
    nodes.push(
      <span className="acp-stream-new" key={`tail:${streamTail.key}`}>
        {last.slice(split)}
      </span>,
    );
    return nodes;
  };
  const formatted = (text: string, blockIndex: number, itemIndex?: number) => {
    const spans = formatsByLocation.get(`${blockIndex}:${itemIndex}`) ?? [];
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
              // Only a cell holding an unbreakable run may break mid-word.
              className: /\S{25}/.test(cell)
                ? "markdown-table__unbroken"
                : undefined,
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
        aria-label={
          block.rows[0].slice(0, 3).join(", ").slice(0, 128) || "Table"
        }
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
