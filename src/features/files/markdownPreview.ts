import { editorLanguageForPath } from "./editorLanguage";

export const MAX_MARKDOWN_PREVIEW_BLOCKS = 64;
export const MAX_MARKDOWN_PREVIEW_BLOCK_CHARS = 4096;
export const MAX_MARKDOWN_PREVIEW_SOURCE_BYTES = 256 * 1024;
export const MAX_CHAT_FILE_LINKS = 8;
export const MAX_CHAT_FILE_LINK_PATH_BYTES = 4096;
import { utf8Bytes } from "../../domain/text";

export type MarkdownPreviewBlock =
  | { kind: "heading"; level: 1 | 2 | 3; text: string }
  | { kind: "paragraph"; text: string }
  | { kind: "list"; items: readonly string[] }
  | { kind: "code"; text: string; language: string; fenceClosed: boolean }
  | { kind: "quote"; text: string };

export interface MarkdownPreviewDocument {
  blocks: readonly MarkdownPreviewBlock[];
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
  const lower = path.toLowerCase();
  return lower.endsWith(".mmd") || lower.endsWith(".mermaid");
}

export function renderMarkdownPreview(source: string): MarkdownPreviewDocument {
  const windowed = sourceWindow(source);
  const lines = stripHostileMarkup(windowed.text)
    .replace(/\r\n/gu, "\n")
    .split("\n");
  const blocks: MarkdownPreviewBlock[] = [];
  const fileLinks: {
    label: string;
    destination: string;
    blockIndex: number;
    itemIndex?: number;
    offset: number;
  }[] = [];
  let truncated = false;
  let paragraph: string[] = [];
  let list: string[] = [];
  let quote: string[] = [];
  let code: { language: string; lines: string[] } | null = null;

  const stop = { value: false };

  const collectFileLinks = (
    raw: string,
    visible: string,
    blockIndex: number,
    itemIndex?: number,
  ) => {
    if (fileLinks.length === MAX_CHAT_FILE_LINKS) return;
    for (const match of raw.matchAll(
      /(?<!!)\[([^\]\n]{1,128})\]\(([^)\n]+)\)/gu,
    )) {
      const destination = match[2];
      if (!fileLinkCandidate(destination)) continue;
      const offset = visible.indexOf(
        match[1],
        visibleInlineText(raw.slice(0, match.index)).length,
      );
      if (offset < 0) continue;
      fileLinks.push({
        label: match[1],
        destination,
        blockIndex,
        itemIndex,
        offset,
      });
      if (fileLinks.length === MAX_CHAT_FILE_LINKS) break;
    }
  };

  const flushParagraph = () => {
    const raw = paragraph.join(" ");
    const text = visibleInlineText(raw);
    paragraph = [];
    if (text.length > 0) {
      collectFileLinks(raw, text, blocks.length);
      truncated = pushBlock(
        blocks,
        { kind: "paragraph", text },
        truncated,
        stop,
      );
    }
  };
  const flushList = () => {
    if (list.length === 0) {
      return;
    }
    const items = list.map((item) => visibleInlineText(item));
    list.forEach((item, index) =>
      collectFileLinks(item, items[index], blocks.length, index),
    );
    list = [];
    truncated = pushBlock(blocks, { kind: "list", items }, truncated, stop);
  };
  const flushQuote = () => {
    const raw = quote.join(" ");
    const text = visibleInlineText(raw);
    quote = [];
    if (text.length > 0) {
      collectFileLinks(raw, text, blocks.length);
      truncated = pushBlock(blocks, { kind: "quote", text }, truncated, stop);
    }
  };
  const flushFlow = () => {
    flushParagraph();
    flushList();
    flushQuote();
  };

  for (const rawLine of lines) {
    if (stop.value) {
      truncated = true;
      break;
    }
    const line = rawLine.replace(/\s+$/u, "");
    if (code) {
      if (/^```/u.test(line)) {
        truncated = pushBlock(
          blocks,
          {
            kind: "code",
            text: code.lines.join("\n"),
            language: code.language,
            fenceClosed: true,
          },
          truncated,
          stop,
        );
        code = null;
      } else {
        code.lines.push(rawLine.replace(/\r$/u, ""));
      }
      continue;
    }
    if (/^```/u.test(line)) {
      flushFlow();
      const language = line.slice(3).trim().split(/\s+/u)[0] ?? "";
      code = { language, lines: [] };
      continue;
    }
    if (line.length === 0) {
      flushFlow();
      continue;
    }
    const heading = /^(#{1,3}) (.+)$/u.exec(line);
    if (heading) {
      flushFlow();
      const level = heading[1].length as 1 | 2 | 3;
      collectFileLinks(
        heading[2],
        visibleInlineText(heading[2]),
        blocks.length,
      );
      truncated = pushBlock(
        blocks,
        { kind: "heading", level, text: visibleInlineText(heading[2]) },
        truncated,
        stop,
      );
      continue;
    }
    const item = /^[-*] (.+)$/u.exec(line);
    if (item) {
      flushParagraph();
      flushQuote();
      list.push(item[1]);
      continue;
    }
    const quoted = /^> (.+)$/u.exec(line);
    if (quoted) {
      flushParagraph();
      flushList();
      quote.push(quoted[1]);
      continue;
    }
    flushList();
    flushQuote();
    paragraph.push(line);
  }

  if (code) {
    truncated = pushBlock(
      blocks,
      {
        kind: "code",
        text: code.lines.join("\n"),
        language: code.language,
        fenceClosed: false,
      },
      truncated,
      stop,
    );
  }
  if (!stop.value) {
    flushFlow();
  }

  return {
    blocks,
    fileLinks,
    truncated,
    sourceLimitExceeded: windowed.exceeded,
  };
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
    path.includes("?") ||
    path.includes("#") ||
    path.split(/[\\/]/u).includes("..") ||
    Array.from(path).some(
      (character) =>
        character.charCodeAt(0) <= 31 || character.charCodeAt(0) === 127,
    )
  )
    return false;
  const scheme = /^[a-z][a-z0-9+.-]*:/iu.test(path);
  return !scheme || /^[a-z]:[\\/]/iu.test(path);
}

function sourceWindow(source: string): { text: string; exceeded: boolean } {
  const encoded = utf8Bytes(source);
  if (encoded.length <= MAX_MARKDOWN_PREVIEW_SOURCE_BYTES) {
    return { text: source, exceeded: false };
  }
  let end = MAX_MARKDOWN_PREVIEW_SOURCE_BYTES;
  while (end > 0 && (encoded[end] & 0b1100_0000) === 0b1000_0000) {
    end -= 1;
  }
  return {
    text: new TextDecoder().decode(encoded.slice(0, end)),
    exceeded: true,
  };
}

function pushBlock(
  blocks: MarkdownPreviewBlock[],
  block: MarkdownPreviewBlock,
  truncated: boolean,
  stop: { value: boolean },
): boolean {
  if (blocks.length >= MAX_MARKDOWN_PREVIEW_BLOCKS) {
    stop.value = true;
    return true;
  }
  const clamped = clampBlock(block);
  blocks.push(clamped.block);
  if (clamped.truncated) {
    return true;
  }
  return truncated;
}

function clampBlock(block: MarkdownPreviewBlock): {
  block: MarkdownPreviewBlock;
  truncated: boolean;
} {
  if (block.kind === "list") {
    return clampList(block.items);
  }
  const clamped = clampText(block.text);
  return {
    block: { ...block, text: clamped.text },
    truncated: clamped.truncated,
  };
}

function clampList(items: readonly string[]): {
  block: MarkdownPreviewBlock;
  truncated: boolean;
} {
  const clamped: string[] = [];
  let remaining = MAX_MARKDOWN_PREVIEW_BLOCK_CHARS;
  let truncated = false;
  for (const item of items) {
    if (remaining <= 0) {
      truncated = true;
      break;
    }
    if (item.length <= remaining) {
      clamped.push(item);
      remaining -= item.length;
      continue;
    }
    clamped.push(item.slice(0, remaining));
    truncated = true;
    remaining = 0;
  }
  return { block: { kind: "list", items: clamped }, truncated };
}

function clampText(value: string): { text: string; truncated: boolean } {
  if (value.length <= MAX_MARKDOWN_PREVIEW_BLOCK_CHARS) {
    return { text: value, truncated: false };
  }
  return {
    text: value.slice(0, MAX_MARKDOWN_PREVIEW_BLOCK_CHARS),
    truncated: true,
  };
}

function stripHostileMarkup(source: string): string {
  return source
    .replace(/<script\b[^>]*>[\s\S]*?<\/script>/giu, "")
    .replace(/<style\b[^>]*>[\s\S]*?<\/style>/giu, "")
    .replace(/<script\b[^>]*>[\s\S]*$/giu, "")
    .replace(/<style\b[^>]*>[\s\S]*$/giu, "");
}

function visibleInlineText(value: string): string {
  let text = stripHostileMarkup(value);
  text = text.replace(/!\[([^\]]*)\]\([^)]*\)/gu, "$1");
  text = text.replace(/\[([^\]]+)\]\([^)]*\)/gu, "$1");
  text = text.replace(/<[^>]+>/gu, "");
  text = text.replace(/\*\*([^*]+)\*\*/gu, "$1");
  text = text.replace(/\*([^*]+)\*/gu, "$1");
  text = text.replace(/`([^`]+)`/gu, "$1");
  return text.replace(/\s+/gu, " ").trim();
}
