// Marks the match in a Search result preview line, as Search found it.

export function highlightPreview(
  preview: string,
  query: string,
  caseSensitive: boolean,
  wholeWord: boolean,
  occurrenceIndex: number,
  useRegex = false,
) {
  const source = preview || "(empty line)";
  if (!query) return source;
  const ranges = useRegex
    ? regexPreviewMatchRanges(source, query, caseSensitive)
    : literalPreviewMatchRanges(source, query, caseSensitive, wholeWord);
  const target = ranges[occurrenceIndex];
  if (!target) return source;
  return (
    <>
      {source.slice(0, target.start)}
      <mark>{source.slice(target.start, target.end)}</mark>
      {source.slice(target.end)}
    </>
  );
}

interface PreviewMatchRange {
  start: number;
  end: number;
}

/**
 * Marks regex matches in a preview line. Search ran the Rust engine; this
 * is a JavaScript RegExp, so a pattern it cannot read just goes unmarked.
 */
function regexPreviewMatchRanges(
  source: string,
  pattern: string,
  caseSensitive: boolean,
): PreviewMatchRange[] {
  let expression: RegExp;
  try {
    expression = new RegExp(pattern, caseSensitive ? "gu" : "giu");
  } catch {
    return [];
  }
  const ranges: PreviewMatchRange[] = [];
  for (const found of source.matchAll(expression)) {
    if (!found[0]) continue;
    ranges.push({ start: found.index, end: found.index + found[0].length });
  }
  return ranges;
}

const wordCharacterPattern = /^[\p{L}\p{N}_]$/u;

function isWordCharacter(character: string | undefined) {
  return character !== undefined && wordCharacterPattern.test(character);
}

function hasWordBoundaries(source: string, { start, end }: PreviewMatchRange) {
  const before = [...source.slice(0, start)].at(-1);
  const after = [...source.slice(end)][0];
  return !isWordCharacter(before) && !isWordCharacter(after);
}

function foldWithOrigin(source: string) {
  let folded = "";
  const origin: PreviewMatchRange[] = [];
  for (let start = 0; start < source.length;) {
    const codePoint = source.codePointAt(start);
    if (codePoint === undefined) break;
    const character = String.fromCodePoint(codePoint);
    const end = start + character.length;
    const foldedCharacter = character.toLowerCase();
    folded += foldedCharacter;
    for (let index = 0; index < foldedCharacter.length; index += 1) {
      origin.push({ start, end });
    }
    start = end;
  }
  return { folded, origin };
}

function literalPreviewMatchRanges(
  source: string,
  query: string,
  caseSensitive: boolean,
  wholeWord: boolean,
) {
  const foldedSource = caseSensitive
    ? { folded: source, origin: null }
    : foldWithOrigin(source);
  const foldedQuery = caseSensitive ? query : foldWithOrigin(query).folded;
  if (!foldedQuery) return [];

  const ranges: PreviewMatchRange[] = [];
  let offset = 0;
  while (offset <= foldedSource.folded.length - foldedQuery.length) {
    const foldedStart = foldedSource.folded.indexOf(foldedQuery, offset);
    if (foldedStart < 0) break;
    const foldedEnd = foldedStart + foldedQuery.length;
    const range = foldedSource.origin
      ? {
          start: foldedSource.origin[foldedStart].start,
          end: foldedSource.origin[foldedEnd - 1].end,
        }
      : { start: foldedStart, end: foldedEnd };
    if (!wholeWord || hasWordBoundaries(source, range)) ranges.push(range);
    offset = foldedEnd;
  }
  return ranges;
}
