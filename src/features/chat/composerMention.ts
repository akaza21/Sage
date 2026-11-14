// Native resolves each token on send (`prompt_mentions.rs`), so the token
// syntax is the whole contract.

export interface MentionQuery {
  start: number;
  end: number;
  /** What follows the `@`, without its quotes. */
  query: string;
}

export interface MentionRoot {
  id: string;
  name: string;
}

const SPACE = /\s/u;

function startsToken(text: string, index: number) {
  const before = text.charAt(index - 1);
  return (
    !before ||
    SPACE.test(before) ||
    (before === "(" && !text.charAt(index - 2).trim())
  );
}

// A null caret means the end.
export function mentionQuery(
  text: string,
  caret: number | null,
): MentionQuery | null {
  const at = caret ?? text.length;
  for (let start = at - 1; start >= 0 && text[start] !== "\n"; start -= 1) {
    if (text[start] !== "@" || !startsToken(text, start)) continue;
    const typed = text.slice(start + 1, at);
    const quoted = typed.startsWith('"');
    if (quoted ? typed.includes('"', 1) : SPACE.test(typed)) return null;
    const rest = text.slice(at);
    const tail =
      (quoted && /^[^"\n]*"/u.exec(rest)?.[0]) || /^\S*/u.exec(rest)![0];
    let end = at + tail.length;
    if (!quoted && text[start - 1] === "(" && tail.endsWith(")")) end -= 1;
    const query = text.slice(start + 1, end).replace(/^"|"$/gu, "");
    return { start, end, query };
  }
  return null;
}

export function mentionRootQualifier(
  roots: readonly MentionRoot[],
  rootId: string,
): string | null {
  if (!roots.length || roots[0]!.id === rootId) return null;
  const name = roots.find((root) => root.id === rootId)?.name ?? ":";
  const shared = roots.filter((root) => root.name === name).length > 1;
  return shared || /[:"]/u.test(name) ? rootId : name;
}

// Null when no token can name the path: a `:` in it, or a `"` a quote would
// hold.
export function mentionToken(
  relativePath: string,
  qualifier: string | null = null,
): string | null {
  if (relativePath.includes(":")) return null;
  const body = qualifier ? `${qualifier}:${relativePath}` : relativePath;
  if (!SPACE.test(body) && !body.startsWith('"')) return `@${body}`;
  return body.includes('"') ? null : `@"${body}"`;
}
