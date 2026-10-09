import { editorLanguageForPath } from "./editorLanguage";
import { utf8Bytes } from "../../domain/text";

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
export {
  MAX_MARKDOWN_PREVIEW_BLOCKS,
  MAX_MARKDOWN_PREVIEW_BLOCK_CHARS,
  MAX_MARKDOWN_PREVIEW_SOURCE_BYTES,
  MAX_MARKDOWN_TABLE_ROWS,
  MAX_MARKDOWN_TABLE_COLUMNS,
  MAX_MARKDOWN_INLINE_FORMATS,
  MAX_CHAT_FILE_LINKS,
  MAX_CHAT_FILE_LINK_PATH_BYTES,
} from "./markdownLimits";

export type MarkdownPreviewBlock =
  | { kind: "heading"; level: 1 | 2 | 3 | 4 | 5 | 6; text: string }
  | { kind: "paragraph"; text: string }
  | { kind: "quote"; text: string }
  | { kind: "list"; items: readonly string[]; ordered?: true; start?: number }
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
    };

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

export function canPreviewMarkdown(path: string): boolean {
  return editorLanguageForPath(path) === "markdown";
}

export function canPreviewDiagram(path: string): boolean {
  return /\.(?:mmd|mermaid)$/iu.test(path);
}

const inlinePattern =
  /`([^`]+)`|(\*\*|__)(.+?)\2(?![*_])|(\*|_)(.+?)\4(?![*_])|(!?)\[([^\]\n]{1,128})\]\(([^)\n]+)\)/gu;

export function renderMarkdownPreview(source: string): MarkdownPreviewDocument {
  const encoded = utf8Bytes(source);
  let end = Math.min(encoded.length, MAX_MARKDOWN_PREVIEW_SOURCE_BYTES);
  while (end < encoded.length && end > 0 && (encoded[end] & 0xc0) === 0x80)
    end--;
  const windowed =
    encoded.length > end
      ? new TextDecoder().decode(encoded.slice(0, end))
      : source;
  const lines = windowed
    .replace(
      /(^(`{3,})[^\n]*\n[\s\S]*?(?:^\2`*[ \t]*\r?$|(?![\s\S])))|`[^`\n]+`|<(script|style)\b[^>]*>[\s\S]*?(?:<\/\3>|(?![\s\S]))/gimu,
      (part) => (part.startsWith("<") ? "" : part),
    )
    .split("\n");
  const document: MarkdownPreviewDocument = {
    blocks: [],
    formats: [],
    fileLinks: [],
    truncated: false,
    sourceLimitExceeded: encoded.length > end,
  };
  const blocks = document.blocks as MarkdownPreviewBlock[];
  const links =
    document.fileLinks as MarkdownPreviewDocument["fileLinks"][number][];
  let remaining = MAX_MARKDOWN_PREVIEW_BLOCK_CHARS;
  let formatCount = 0;

  const inline = (raw: string, itemIndex?: number) => {
    let text = "";
    const append = (value: string) => {
      if (text.length + value.length > remaining) document.truncated = true;
      const retained = value.slice(0, Math.max(0, remaining - text.length));
      text += retained;
      return retained;
    };
    const parse = (value: string, depth: number): InlineFormat[] => {
      const spans: InlineFormat[] = [];
      let position = 0;
      for (const match of value.matchAll(inlinePattern)) {
        spans.push(append(value.slice(position, match.index)));
        const from = text.length;
        if (match[7] !== undefined) {
          const label = append(match[7]);
          if (
            !match[6] &&
            links.length < MAX_CHAT_FILE_LINKS &&
            text.length - from === match[7].length &&
            fileLinkCandidate(match[8])
          ) {
            spans.push({ link: links.length, label });
            links.push({
              label: match[7],
              destination: match[8],
              blockIndex: blocks.length,
              itemIndex,
              offset: from,
            });
          } else spans.push(label);
        } else if (depth < 8 && formatCount < MAX_MARKDOWN_INLINE_FORMATS) {
          formatCount++;
          const tag =
            match[1] !== undefined ? "code" : match[2] ? "strong" : "em";
          const children =
            tag === "code"
              ? [append(match[1])]
              : parse(match[3] ?? match[5], depth + 1);
          spans.push({ tag, children });
        } else spans.push(append(match[1] ?? match[3] ?? match[5]));
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
        .replace(/`[^`]+`|<[^>]+>|\s+/gu, (part) =>
          part.startsWith("`") ? part : part.startsWith("<") ? "" : " ",
        )
        .trim(),
      0,
    );
    if (text)
      document.formats.push({ blockIndex: blocks.length, itemIndex, spans });
    remaining -= text.length;
    return text;
  };

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
    const fence = /^(`{3,})(.*)$/u.exec(line);
    if (fence) {
      const language = fence[2].trim().split(/\s/u)[0];
      const closing = new RegExp(`^\`{${fence[1].length},}[ \\t]*\\r?$`, "u");
      const start = ++cursor;
      while (cursor < lines.length && !closing.test(lines[cursor])) cursor++;
      const fenceClosed = cursor < lines.length;
      const text = lines.slice(start, cursor).join("\n").replace(/\r$/u, "");
      if (text.length > remaining) document.truncated = true;
      blocks.push({
        kind: "code",
        language,
        text: text.slice(0, remaining),
        fenceClosed,
      });
      if (fenceClosed) cursor++;
      continue;
    }
    const heading = /^(#{1,6}) (.+)$/u.exec(line);
    if (heading) {
      blocks.push({
        kind: "heading",
        level: heading[1].length as 1,
        text: inline(heading[2]),
      });
      cursor++;
      continue;
    }
    const header = tableCells(line);
    const separator = tableCells(lines[cursor + 1] ?? "");
    if (
      header.length &&
      separator.length === header.length &&
      separator.every((cell) => /^:?-{3,}:?$/u.test(cell))
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
    const item = /^(?:([-*]) |(\d{1,9})[.)] )(.+)$/u.exec(line);
    if (item) {
      const ordered = !!item[2];
      const items: string[] = [];
      let next = item;
      do {
        if (remaining > 0) {
          const text = inline(next[3], items.length);
          if (text) items.push(text);
        } else document.truncated = true;
        cursor++;
        next = /^(?:([-*]) |(\d{1,9})[.)] )(.+)$/u.exec(lines[cursor] ?? "")!;
      } while (next && !!next[2] === ordered);
      blocks.push({
        kind: "list",
        items,
        ...(ordered ? { ordered: true, start: Number(item[2]) } : {}),
      });
      continue;
    }
    const quoted = line.startsWith("> ");
    const parts = [quoted ? line.slice(2) : line];
    cursor++;
    while (
      cursor < lines.length &&
      lines[cursor].trim() &&
      (quoted
        ? lines[cursor].startsWith("> ")
        : !/^(?:```|#{1,6} |[-*] |\d{1,9}[.)] |> |\|)/u.test(lines[cursor]))
    ) {
      parts.push(quoted ? lines[cursor].slice(2) : lines[cursor]);
      cursor++;
    }
    const text = inline(parts.join(" "));
    if (text) blocks.push({ kind: quoted ? "quote" : "paragraph", text });
  }
  return document;
}

function tableCells(line: string): string[] {
  if (!line.includes("|")) return [];
  return line
    .trim()
    .replace(/^\|/u, "")
    .replace(/(?<!\\)\|$/u, "")
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
