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
   * A streamed answer's still-fading runs: the lengths of a contiguous
   * suffix of one block, item or cell, oldest first. Each is wrapped, keyed
   * by where it starts, so it keeps its own fade while newer runs land.
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
    // Only a trailing plain-text run is split; formatted ends stay whole.
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
  return document.blocks.map((block, index) => {
    if (block.kind === "rule") return <hr key={index} />;
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
    if (block.kind === "list") {
      const shape = (at: number) =>
        block.shape?.[at] ?? { depth: 0, ordered: !!block.ordered };
      // Items are flat; each run of deeper items nests in the item before it.
      // The first item of a run is at the run's depth, so `items` is never
      // empty when a deeper item arrives.
      const list = (from: number, key: number): [ReactNode, number] => {
        const { depth, ordered } = shape(from);
        const items: { task: boolean; children: ReactNode[] }[] = [];
        let at = from;
        while (at < block.items.length && shape(at).depth >= depth) {
          if (shape(at).depth > depth) {
            const [nested, next] = list(at, at);
            items[items.length - 1].children.push(nested);
            at = next;
            continue;
          }
          const checked = shape(at).checked;
          items.push({
            task: checked !== undefined,
            children: [
              checked === undefined ? null : (
                <input
                  key="task"
                  type="checkbox"
                  checked={checked}
                  disabled
                  aria-label={block.items[at]}
                />
              ),
              <Fragment key="text">
                {inline(block.items[at], index, at)}
              </Fragment>,
            ],
          });
          at++;
        }
        return [
          createElement(
            ordered ? "ol" : "ul",
            { key, start: depth ? undefined : block.start },
            items.map((item, itemIndex) => (
              <li
                key={itemIndex}
                className={item.task ? "markdown-task" : undefined}
              >
                {item.children}
              </li>
            )),
          ),
          at,
        ];
      };
      const lists: ReactNode[] = [];
      for (let at = 0; at < block.items.length;) {
        const [node, next] = list(at, at);
        lists.push(node);
        at = next;
      }
      return <Fragment key={index}>{lists}</Fragment>;
    }
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
