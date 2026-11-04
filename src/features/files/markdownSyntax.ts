/** Columns, rather than characters, govern Markdown container indentation. */
export function columnAfter(text: string, column = 0): number {
  for (const character of text)
    column += character === "\t" ? 4 - (column % 4) : 1;
  return column;
}

export function indentation(line: string): number {
  return columnAfter(/^[ \t]*/u.exec(line)![0]);
}

/** Removing part of a tab leaves its remaining columns as spaces. */
export function stripIndent(line: string, columns: number): string {
  let column = 0;
  let cursor = 0;
  while (
    cursor < line.length &&
    column < columns &&
    /[ \t]/u.test(line[cursor])
  )
    column = columnAfter(line[cursor++], column);
  return " ".repeat(Math.max(0, column - columns)) + line.slice(cursor);
}

export const setextUnderline = /^ {0,3}(=+|-+)[ \t]*$/u;
export const quoteMarker = /^ {0,3}>[ \t]?/u;
export const atxHeading = /^ {0,3}(#{1,6})(?:[ \t]+(.*))?$/u;

export function fenceOpener(line: string): RegExpExecArray | null {
  const fence = /^( {0,3})(`{3,}|~{3,})(.*)$/u.exec(line);
  return fence?.[2][0] === "`" && fence[3].includes("`") ? null : fence;
}

export function fenceCloser(fence: RegExpExecArray): RegExp {
  return new RegExp(`^ {0,3}${fence[2][0]}{${fence[2].length},}[ \\t]*$`, "u");
}

/** Fence indentation is relative to the enclosing item's content column. */
export function readFencedCode(
  lines: readonly string[],
  cursor: number,
  contentIndent: number,
) {
  const line = lines[cursor];
  const base =
    contentIndent && indentation(line) >= contentIndent ? contentIndent : 0;
  const fence = fenceOpener(stripIndent(line, base));
  if (!fence) return null;
  const closing = fenceCloser(fence);
  const start = ++cursor;
  while (
    cursor < lines.length &&
    !(
      indentation(lines[cursor]) >= base &&
      closing.test(stripIndent(lines[cursor], base))
    )
  )
    cursor++;
  const fenceClosed = cursor < lines.length;
  const text = lines
    .slice(start, cursor)
    .map((part) => stripIndent(part, base + fence[1].length))
    .join("\n");
  if (fenceClosed) cursor++;
  return {
    block: {
      kind: "code" as const,
      language: fence[3].trim().split(/\s/u)[0],
      text,
      fenceClosed,
    },
    cursor,
  };
}

/** A thematic break takes precedence over a list marker. */
export function isMarkdownRule(line: string): boolean {
  return /^ {0,3}([-*_])(?:[ \t]*\1){2,}[ \t]*$/u.test(line);
}

export function startsLeafBlock(line: string): boolean {
  return !!(
    fenceOpener(line) ||
    atxHeading.test(line) ||
    quoteMarker.test(line) ||
    isMarkdownRule(line)
  );
}
