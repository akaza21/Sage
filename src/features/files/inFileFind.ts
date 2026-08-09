export const MAX_FIND_QUERY_LENGTH = 256;
export const MAX_FIND_MATCHES = 10_000;

export interface FindMatches {
  offsets: number[];
  truncated: boolean;
}

function escapeRegularExpression(value: string): string {
  return value.replace(/[.*+?^${}()|[\]\\]/gu, "\\$&");
}

export function findLiteralMatches(
  content: string,
  query: string,
  caseSensitive: boolean,
): FindMatches {
  if (query.length === 0 || query.length > MAX_FIND_QUERY_LENGTH) {
    return { offsets: [], truncated: false };
  }

  const expression = new RegExp(
    escapeRegularExpression(query),
    caseSensitive ? "gu" : "giu",
  );
  const offsets: number[] = [];
  for (const match of content.matchAll(expression)) {
    if (offsets.length === MAX_FIND_MATCHES) {
      return { offsets, truncated: true };
    }
    offsets.push(match.index);
  }
  return { offsets, truncated: false };
}
