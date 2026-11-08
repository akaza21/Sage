/**
 * The composer's `@` file mentions: which text opens the picker and what a
 * pick writes. A mention is plain prompt text, `@path` or `@"path with
 * spaces"`, as Claude Code reads it; native resolves each token in the
 * project's roots on send and attaches the file it names
 * (`prompt_mentions.rs`), so the token is the whole contract.
 */

export interface MentionQuery {
  /** Where the `@` token starts and ends, for replacing it. */
  start: number;
  end: number;
  /** What follows the `@`, without an opening quote. */
  query: string;
}

const SPACE = /\s/u;

/**
 * The `@` token the caret is in, when it starts the text or follows
 * whitespace (so `name@example.com` is not one). A null caret means the end.
 */
export function mentionQuery(
  text: string,
  caret: number | null,
): MentionQuery | null {
  const at = caret ?? text.length;
  let start = at;
  while (start > 0 && !SPACE.test(text.charAt(start - 1))) start -= 1;
  if (text.charAt(start) !== "@" || at === start) return null;
  let end = at;
  while (end < text.length && !SPACE.test(text.charAt(end))) end += 1;
  const token = text.slice(start + 1, end);
  return { start, end, query: token.replace(/^"|"$/gu, "") };
}

/** The token a pick writes; a path with whitespace is quoted. */
export function mentionToken(relativePath: string): string {
  return SPACE.test(relativePath) ? `@"${relativePath}"` : `@${relativePath}`;
}
