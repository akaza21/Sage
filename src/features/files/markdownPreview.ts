import { truncateUtf8, utf8Bytes } from "../../domain/text";
import * as lists from "./markdownList";
import {
  atxHeading,
  isMarkdownRule,
  quoteMarker,
  fenceOpener,
  fenceCloser,
  indentation,
  setextUnderline,
  startsLeafBlock,
  readFencedCode,
} from "./markdownSyntax";

import {
  MAX_MARKDOWN_PREVIEW_BLOCKS,
  MAX_MARKDOWN_PREVIEW_BLOCK_CHARS,
  MAX_MARKDOWN_PREVIEW_SOURCE_BYTES,
  MAX_MARKDOWN_TABLE_ROWS,
  MAX_MARKDOWN_TABLE_COLUMNS,
  MAX_MARKDOWN_INLINE_FORMATS,
  MAX_CHAT_FILE_LINKS,
  MAX_CHAT_FILE_LINK_PATH_BYTES,
} from "./markdownLimits";
export type { ListItemShape } from "./markdownList";
export {
  MAX_MARKDOWN_PREVIEW_BLOCKS,
  MAX_MARKDOWN_PREVIEW_BLOCK_CHARS,
  MAX_MARKDOWN_PREVIEW_SOURCE_BYTES,
  MAX_MARKDOWN_TABLE_ROWS,
  MAX_MARKDOWN_TABLE_COLUMNS,
  MAX_MARKDOWN_INLINE_FORMATS,
  MAX_MARKDOWN_LIST_DEPTH,
  MAX_CHAT_FILE_LINKS,
  MAX_CHAT_FILE_LINK_PATH_BYTES,
} from "./markdownLimits";

export type MarkdownPreviewBlock =
  | { kind: "rule" }
  | { kind: "heading"; level: 1 | 2 | 3 | 4 | 5 | 6; text: string }
  | { kind: "paragraph"; text: string }
  | { kind: "quote"; text: string }
  | lists.MarkdownListBlock
  | { kind: "code"; text: string; language: string; fenceClosed: boolean }
  | {
      kind: "table";
      align: ("left" | "center" | "right" | undefined)[];
      rows: string[][];
    };

export type InlineFormat =
  | string
  | { link: number; label: string }
  | {
      tag: "strong" | "em" | "code";
      children: InlineFormat[];
    }
  | { tag: "a"; href: string; children: InlineFormat[] };

export interface MarkdownPreviewDocument {
  blocks: readonly MarkdownPreviewBlock[];
  formats: { blockIndex: number; itemIndex?: number; spans: InlineFormat[] }[];
  fileLinks: readonly {
    label: string;
    destination: string;
    blockIndex: number;
    itemIndex?: number;
    offset: number;
  }[];
  truncated: boolean;
  sourceLimitExceeded: boolean;
}

const inlinePattern =
  /`([^`]+)`|(!?)\[([^\]\n]{1,128})\]\(([^)\n]+)\)|(\*{1,2}|_{1,2})/gu;

function delimiterFlanks(value: string, index: number, marker: string) {
  const characterClass = (character: string) =>
    !character || /\s/u.test(character)
      ? 0
      : /[\p{P}\p{S}]/u.test(character)
        ? 1
        : 2;
  const before = characterClass(value[index - 1] ?? "");
  const after = characterClass(value[index + marker.length] ?? "");
  const left = after > 0 && (after !== 1 || before !== 2);
  const right = before > 0 && (before !== 1 || after !== 2);
  const underscore = marker[0] === "_";
  return [
    left && (!underscore || !right || before === 1),
    right && (!underscore || !left || after === 1),
  ];
}

export function renderMarkdownPreview(source: string): MarkdownPreviewDocument {
  const windowed = truncateUtf8(source, MAX_MARKDOWN_PREVIEW_SOURCE_BYTES);
  const lines = windowed.replace(/\r\n/gu, "\n").split("\n");
  const document: MarkdownPreviewDocument = {
    blocks: [],
    formats: [],
    fileLinks: [],
    truncated: false,
    sourceLimitExceeded: windowed.length < source.length,
  };
  const blocks = document.blocks as MarkdownPreviewBlock[];
  const links =
    document.fileLinks as MarkdownPreviewDocument["fileLinks"][number][];
  let remaining = MAX_MARKDOWN_PREVIEW_BLOCK_CHARS;
  let formatCount = 0;
  let scanBudget = MAX_MARKDOWN_PREVIEW_SOURCE_BYTES;

  const inline = (raw: string, itemIndex?: number) => {
    let text = "";
    const append = (value: string) => {
      if (text.length + value.length > remaining) document.truncated = true;
      const retained = value.slice(0, remaining - text.length);
      text += retained;
      return retained;
    };
    const parse = (value: string, depth: number): InlineFormat[] => {
      const spans: InlineFormat[] = [];
      let position = 0;
      const pattern = new RegExp(inlinePattern);
      let match: RegExpExecArray | null;
      scan: while ((match = pattern.exec(value))) {
        const marker = match[5];
        let content = match[1] ?? "";
        if (marker) {
          const opening = delimiterFlanks(value, match.index, marker);
          let close = opening[0]
            ? value.indexOf(marker, pattern.lastIndex)
            : -1;
          while (close >= 0) {
            if ((scanBudget -= close - match.index) < 0) {
              document.truncated = true;
              break scan;
            }
            const closing = delimiterFlanks(value, close, marker);
            content = value.slice(match.index + marker.length, close);
            const prose = content.replace(/`[^`]*`/gu, "");
            const label = prose.lastIndexOf("[");
            if (
              closing[1] &&
              value[close + marker.length] !== marker[0] &&
              (content.match(/`/gu)?.length ?? 0) % 2 === 0 &&
              !(
                label > prose.lastIndexOf("]") ||
                (label >= 0 && prose.lastIndexOf("](") > prose.lastIndexOf(")"))
              )
            ) {
              match[0] = value.slice(match.index, close + marker.length);
              pattern.lastIndex = close + marker.length;
              break;
            }
            close = value.indexOf(marker, close + 1);
          }
          if (close < 0) {
            pattern.lastIndex = match.index + marker.length;
            continue;
          }
        }
        spans.push(append(value.slice(position, match.index)));
        const from = text.length;
        if (match[3] !== undefined) {
          const label = append(match[3]);
          // A web link keeps its address for display; it is never followed.
          const href = !match[2] && webLink(match[4]);
          if (
            !match[2] &&
            !href &&
            links.length < MAX_CHAT_FILE_LINKS &&
            text.length - from === match[3].length &&
            fileLinkCandidate(match[4])
          ) {
            spans.push({ link: links.length, label });
            links.push({
              label: match[3],
              destination: match[4],
              blockIndex: blocks.length,
              itemIndex,
              offset: from,
            });
          } else
            spans.push(href ? { tag: "a", href, children: [label] } : label);
        } else if (depth < 8 && formatCount < MAX_MARKDOWN_INLINE_FORMATS) {
          formatCount++;
          const tag = marker ? (marker.length === 2 ? "strong" : "em") : "code";
          const children =
            tag === "code" ? [append(content)] : parse(content, depth + 1);
          spans.push({ tag, children });
        } else spans.push(append(content));
        position = match.index + match[0].length;
        if (text.length === remaining) {
          if (position < value.length) document.truncated = true;
          break;
        }
      }
      spans.push(append(value.slice(position)));
      return spans;
    };
    const spans = parse(
      raw
        .replace(/`[^`]+`|<\/?[a-z][\w-]*(?:\s[^<>]*)?\/?>/giu, (part) =>
          part[0] === "`" ? part : "",
        )
        .replace(/`[^`]+`|\s+/gu, (part) => (part[0] === "`" ? part : " "))
        .trim(),
      0,
    );
    if (text)
      document.formats.push({ blockIndex: blocks.length, itemIndex, spans });
    remaining -= text.length;
    return text;
  };

  let contentIndent = 0;
  for (let cursor = 0; cursor < lines.length;) {
    const line = lines[cursor].trimEnd();
    if (!line) {
      cursor++;
      continue;
    }
    if (blocks.length === MAX_MARKDOWN_PREVIEW_BLOCKS) {
      document.truncated = true;
      break;
    }
    remaining = MAX_MARKDOWN_PREVIEW_BLOCK_CHARS;
    formatCount = 0;
    scanBudget = MAX_MARKDOWN_PREVIEW_SOURCE_BYTES;
    const fence = readFencedCode(lines, cursor, contentIndent);
    contentIndent = 0;
    if (fence) {
      const { text } = fence.block;
      if (text.length > remaining) document.truncated = true;
      blocks.push({
        ...fence.block,
        text: text.slice(0, remaining),
      });
      cursor = fence.cursor;
      continue;
    }
    if (isMarkdownRule(line)) {
      blocks.push({ kind: "rule" });
      cursor++;
      continue;
    }
    const heading = atxHeading.exec(line);
    if (heading) {
      blocks.push({
        kind: "heading",
        level: heading[1].length as 1,
        text: inline(heading[2] ?? ""),
      });
      cursor++;
      continue;
    }
    const header = tableCells(line);
    const separator = tableCells(lines[cursor + 1] ?? "");
    if (
      header.length &&
      separator.length === header.length &&
      separator.every((cell) => /^:?-+:?$/u.test(cell))
    ) {
      const align = separator
        .slice(0, MAX_MARKDOWN_TABLE_COLUMNS)
        .map((cell) =>
          cell.endsWith(":")
            ? cell.startsWith(":")
              ? ("center" as const)
              : ("right" as const)
            : cell.startsWith(":")
              ? ("left" as const)
              : undefined,
        );
      const rows: string[][] = [];
      cursor += 2;
      let cells = header;
      do {
        if (rows.length <= MAX_MARKDOWN_TABLE_ROWS) {
          if (cells.length > MAX_MARKDOWN_TABLE_COLUMNS)
            document.truncated = true;
          const rowIndex = rows.length;
          rows.push(
            align.map((_, column) =>
              inline(cells[column] ?? "", rowIndex * align.length + column),
            ),
          );
        } else document.truncated = true;
        cells = tableCells(lines[cursor] ?? "");
        if (cells.length) cursor++;
      } while (cells.length);
      blocks.push({ kind: "table", align, rows });
      continue;
    }
    const list = lists.readMarkdownList(lines, cursor, (text, itemIndex) => {
      // Empty items still consume a rendered list slot.
      if (remaining > 0 && itemIndex < MAX_MARKDOWN_PREVIEW_BLOCK_CHARS)
        return inline(text, itemIndex);
      document.truncated = true;
      return null;
    });
    if (list) {
      blocks.push(list.block);
      cursor = list.cursor;
      contentIndent = list.contentIndent;
      continue;
    }
    const quoted = quoteMarker.test(line);
    const parts = quoted ? [] : [line];
    if (!quoted) cursor++;
    let paragraph = false;
    let closing: RegExp | null = null;
    while (cursor < lines.length && lines[cursor].trim()) {
      let part = lines[cursor];
      if (quoted && quoteMarker.test(part)) {
        part = part.replace(quoteMarker, "");
        // Literal quote children admit lazy lines only while a paragraph is
        // open. A marked code line cannot reopen one inside a fence.
        if (closing) {
          if (closing.test(part)) closing = null;
          paragraph = false;
        } else {
          const fence = fenceOpener(part);
          closing = fence && fenceCloser(fence);
          paragraph =
            !!part.trim() &&
            (paragraph || indentation(part) < 4) &&
            !startsLeafBlock(part);
        }
      } else if (
        startsLeafBlock(part) ||
        lists.interruptsParagraph(part) ||
        (quoted
          ? !paragraph
          : setextUnderline.test(part) || part.startsWith("|"))
      )
        break;
      parts.push(part);
      cursor++;
    }
    const text = inline(parts.join(" "));
    const underline = !quoted && setextUnderline.exec(lines[cursor] ?? "");
    if (underline) {
      blocks.push({
        kind: "heading",
        level: underline[1][0] === "=" ? 1 : 2,
        text,
      });
      cursor++;
    } else if (text || quoted)
      blocks.push({ kind: quoted ? "quote" : "paragraph", text });
  }
  return document;
}

function webLink(destination: string): string | null {
  const url = destination
    .trim()
    .split(/\s/u)[0]
    .replace(/^<(.*)>$/u, "$1");
  // Control and format characters could disguise the address shown.
  return /^(?:https?:\/\/|mailto:)[^\s<>\p{Cc}\p{Cf}]+$/iu.test(url)
    ? url
    : null;
}

function tableCells(line: string): string[] {
  if (!line.includes("|")) return [];
  return line
    .trim()
    .replace(/^\||(?<!\\)\|$/gu, "")
    .split(/(?<!\\)\|/u)
    .map((cell) => cell.trim().replace(/\\\|/gu, "|"));
}

function fileLinkCandidate(destination: string): boolean {
  if (
    utf8Bytes(destination).length > MAX_CHAT_FILE_LINK_PATH_BYTES ||
    destination.startsWith("//") ||
    destination.startsWith("\\\\")
  )
    return false;
  const path = destination.replace(/:[1-9][0-9]*$/u, "");
  if (
    /[?#]/u.test(path) ||
    path
      .split("")
      .some((character) => character < " " || character === "\u007f") ||
    path.split(/[\\/]/u).includes("..")
  )
    return false;
  return !/^[a-z][a-z0-9+.-]*:/iu.test(path) || /^[a-z]:[\\/]/iu.test(path);
}

export function latestMarkdownLine(source: string): string {
  for (const rawLine of source.split("\n").reverse()) {
    const line = rawLine.trim().replace(/^\d+[.)]\s+/u, "");
    if (!line) continue;
    const block = renderMarkdownPreview(line).blocks[0];
    const text =
      block?.kind === "list"
        ? block.items[0]
        : block && "text" in block
          ? block.text
          : "";
    if (text) return text;
  }
  return "";
}
