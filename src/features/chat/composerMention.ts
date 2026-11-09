/**
 * The composer's `@` file mentions: which text opens the picker and what a
 * pick writes. A mention is plain prompt text, `@path` or `@"path with
 * spaces"`, as Claude Code reads it; a file in a root other than the first
 * is written `@root:path`. Native resolves each token in the project's
 * roots on send and attaches the file it names (`prompt_mentions.rs`), so
 * the token is the whole contract.
 */

export interface MentionQuery {
  /** Where the `@` token starts and ends, for replacing it. */
  start: number;
  end: number;
  /** What follows the `@`, without its quotes. */
  query: string;
}

/** The parts of a project root a token can name it by. */
export interface MentionRoot {
  id: string;
  name: string;
}

const SPACE = /\s/u;

/** Whether an `@` at `index` starts a token, as native reads one. */
function startsToken(text: string, index: number) {
  const before = text.charAt(index - 1);
  return (
    !before ||
    SPACE.test(before) ||
    (before === "(" && !text.charAt(index - 2).trim())
  );
}

/**
 * The `@` token the caret is in, when it starts the text or follows
 * whitespace or an opening parenthesis (so `name@example.com` is not one).
 * A quoted token stays open across its spaces until its closing quote. A
 * null caret means the end.
 */
export function mentionQuery(
  text: string,
  caret: number | null,
): MentionQuery | null {
  const at = caret ?? text.length;
  for (let start = at - 1; start >= 0 && text[start] !== "\n"; start -= 1) {
    if (text[start] !== "@" || !startsToken(text, start)) continue;
    const typed = text.slice(start + 1, at);
    const quoted = typed.startsWith('"');
    // The caret has left the token: past its closing quote or a space.
    if (quoted ? typed.includes('"', 1) : SPACE.test(typed)) return null;
    const rest = text.slice(at);
    const tail =
      (quoted && /^[^"\n]*"/u.exec(rest)?.[0]) || /^\S*/u.exec(rest)![0];
    let end = at + tail.length;
    // `(@a.rs)` keeps its closing parenthesis when a pick replaces the token.
    if (!quoted && text[start - 1] === "(" && tail.endsWith(")")) end -= 1;
    const query = text.slice(start + 1, end).replace(/^"|"$/gu, "");
    return { start, end, query };
  }
  return null;
}

/**
 * How a token names the root `rootId` of `roots` (in root order): nothing
 * for the first, which a bare path reaches, otherwise its display name when
 * no other root shares it and native can read it back, or else its id.
 */
export function mentionRootQualifier(
  roots: readonly MentionRoot[],
  rootId: string,
): string | null {
  if (!roots.length || roots[0]!.id === rootId) return null;
  const name = roots.find((root) => root.id === rootId)?.name ?? ":";
  const shared = roots.filter((root) => root.name === name).length > 1;
  return shared || /[:"]/u.test(name) ? rootId : name;
}

/**
 * The token a pick writes, quoted when it holds whitespace, or null when
 * no token can name the path: a `:` in it, or a `"` a quote would hold.
 */
export function mentionToken(
  relativePath: string,
  qualifier: string | null = null,
): string | null {
  if (relativePath.includes(":")) return null;
  const body = qualifier ? `${qualifier}:${relativePath}` : relativePath;
  if (!SPACE.test(body) && !body.startsWith('"')) return `@${body}`;
  return body.includes('"') ? null : `@"${body}"`;
}
