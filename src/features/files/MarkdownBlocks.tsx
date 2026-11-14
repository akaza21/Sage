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
  streamRuns,
}: {
  document: MarkdownPreviewDocument;
  renderLink?: (index: number) => ReactNode;
  renderCode?: (
    block: Extract<MarkdownPreviewBlock, { kind: "code" }>,
  ) => ReactNode;
  /**
   * Lengths of a contiguous suffix of one block, item or cell, oldest first.
   */
  streamRuns?: {
    blockIndex: number;
    itemIndex: number | undefined;
    runs: readonly number[];
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
      streamRuns?.blockIndex !== blockIndex ||
      streamRuns.itemIndex !== itemIndex
    )
      return content;
    const nodes = Array.isArray(content) ? [...content] : [content];
    const last = nodes.at(-1);
    if (typeof last !== "string" || !last) return content;
    let runs = streamRuns.runs;
    let total = runs.reduce((sum, chars) => sum + chars, 0);
    if (last !== text || total > last.length) {
      total = Math.min(last.length, runs.at(-1) ?? 0);
      runs = [total];
    }
    const boundary = (offset: number) =>
      offset > 0 && /[\uDC00-\uDFFF]/.test(last[offset]) ? offset - 1 : offset;
    let offset = last.length - total;
    let split = boundary(offset);
    nodes[nodes.length - 1] = last.slice(0, split);
    runs.forEach((chars) => {
      const start = split;
      offset += chars;
      split = boundary(offset);
      if (split > start)
        nodes.push(
          <span className="acp-stream-new" key={"tail:" + start}>
            {last.slice(start, split)}
          </span>,
        );
    });
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
            : format.tag === "a"
              ? // Not followed in the app; the address shows on hover.
                createElement(
                  "span",
                  {
                    key: index,
                    className: "markdown-web-link",
                    title: format.href,
                  },
                  render(format.children),
                )
              : createElement(
                  format.tag,
                  { key: index },
                  render(format.children),
                ),
      );
    return spans.length ? render(spans) : text;
  };
  const render = (
    block: MarkdownPreviewBlock,
    index: number,
    key: number,
    location: { slot: number },
    nested = false,
    tight = false,
  ): ReactNode => {
    if (block.kind === "rule") return <hr key={key} />;
    if (block.kind === "code")
      return (
        <div key={key}>
          {renderCode ? (
            renderCode(block)
          ) : block.language.toLowerCase() === "mermaid" ? (
            <MermaidFigureBoundary source={block.text} />
          ) : (
            <pre>{block.text}</pre>
          )}
        </div>
      );
    const children = (blocks: readonly MarkdownPreviewBlock[], tight = false) =>
      blocks.map((child, childIndex) =>
        render(child, index, childIndex, location, true, tight),
      );
    if (block.kind === "quote")
      return <blockquote key={key}>{children(block.children)}</blockquote>;
    if (block.kind === "paragraph" || block.kind === "heading") {
      const text = inline(
        block.text,
        index,
        nested ? location.slot++ : undefined,
      );
      return block.kind === "paragraph" && tight ? (
        <span key={key} data-markdown-inline="">
          {text}
        </span>
      ) : (
        createElement(
          block.kind === "heading" ? `h${block.level}` : "p",
          { key },
          text,
        )
      );
    }
    if (block.kind === "list")
      return createElement(
        block.ordered ? "ol" : "ul",
        { key, start: block.start },
        block.items.map((item, itemIndex) => (
          <li
            key={itemIndex}
            className={item.checked !== undefined ? "markdown-task" : undefined}
          >
            {item.checked !== undefined ? (
              <input
                type="checkbox"
                checked={item.checked}
                disabled
                aria-label={
                  item.children.find((child) => child.kind === "paragraph")
                    ?.text || "Task"
                }
              />
            ) : null}
            {children(item.children, !block.loose)}
          </li>
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
              className: /\S{25}/.test(cell)
                ? "markdown-table__unbroken"
                : undefined,
            },
            inline(
              cell,
              index,
              nested ? location.slot++ : rowIndex * block.align.length + column,
            ),
          ),
        )}
      </tr>
    );
    return (
      <div
        className="markdown-table"
        key={key}
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
  };
  return document.blocks.map((block, index) =>
    render(block, index, index, { slot: 0 }),
  );
}
