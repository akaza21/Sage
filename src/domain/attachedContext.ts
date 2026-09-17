import type { AgentDiffRef } from "./agentDiff";
import { decimalU64Counter, isRecord } from "./guards";
import { utf8ByteLength } from "./text";

/** One turn may carry only a small review set, not an unbounded file annotation log. */
export const MAX_ATTACHED_CONTEXTS_PER_TURN = 8;
/** Matches a quarter of the composer budget and keeps eight notes within 32 KiB. */
export const MAX_ATTACHED_CONTEXT_NOTE_BYTES = 4 * 1024;

export type DiffLineSide = "old" | "new";

function unsafeDisplayCharacter(character: string): boolean {
  const code = character.codePointAt(0) ?? 0;
  return (
    code < 32 ||
    (code >= 0x7f && code <= 0x9f) ||
    code === 0x061c ||
    code === 0x200e ||
    code === 0x200f ||
    code === 0x2028 ||
    code === 0x2029 ||
    (code >= 0x202a && code <= 0x202e) ||
    (code >= 0x2066 && code <= 0x2069)
  );
}

export interface DiffLineCommentContext {
  kind: "diffLineComment";
  diffRef: AgentDiffRef;
  side: DiffLineSide;
  startLine: string;
  endLine: string;
  note: string;
}

function parseAttachedAgentDiffRef(value: unknown): AgentDiffRef | null {
  if (!isRecord(value)) return null;
  const ids = [
    "projectId",
    "chatId",
    "sessionId",
    "turnId",
    "itemId",
    "rootId",
  ] as const;
  const bounded = Object.fromEntries(
    ids.map((key) => [key, typeof value[key] === "string" ? value[key] : null]),
  ) as Record<(typeof ids)[number], string | null>;
  if (
    ids.some((key) => {
      const id = bounded[key];
      return !id || id.includes("\0") || utf8ByteLength(id) > 512;
    })
  ) {
    return null;
  }
  const runtimeEpoch = decimalU64Counter(value.runtimeEpoch, {
    allowZero: false,
  });
  const snapshotRevision = decimalU64Counter(value.snapshotRevision, {
    allowZero: false,
  });
  const path = value.relativePath;
  if (
    !runtimeEpoch ||
    !snapshotRevision ||
    value.diffOrdinal !== 0 ||
    typeof path !== "string" ||
    !path ||
    utf8ByteLength(path) > 16_384 ||
    [...path].some(unsafeDisplayCharacter) ||
    path.startsWith("/") ||
    path.includes("\\") ||
    (path.charCodeAt(0) < 128 && path[1] === ":") ||
    path.split("/").some((part) => !part || part === "." || part === "..") ||
    typeof value.contentDigest !== "string" ||
    !/^[0-9a-f]{64}$/.test(value.contentDigest)
  ) {
    return null;
  }
  return {
    ...bounded,
    runtimeEpoch,
    snapshotRevision,
    diffOrdinal: 0,
    relativePath: path,
    contentDigest: value.contentDigest,
  } as AgentDiffRef;
}

export function parseDiffLineCommentContext(
  value: unknown,
): DiffLineCommentContext | null {
  if (!isRecord(value) || value.kind !== "diffLineComment") return null;
  const diffRef = parseAttachedAgentDiffRef(value.diffRef);
  const startLine = decimalU64Counter(value.startLine, { allowZero: false });
  const endLine = decimalU64Counter(value.endLine, { allowZero: false });
  const rawNote = typeof value.note === "string" ? value.note : "";
  if ([...rawNote].some(unsafeDisplayCharacter)) return null;
  const note = rawNote.trim();
  if (
    !diffRef ||
    (value.side !== "old" && value.side !== "new") ||
    !startLine ||
    !endLine ||
    BigInt(startLine) > BigInt(endLine) ||
    !note ||
    [...note].some(unsafeDisplayCharacter) ||
    utf8ByteLength(note) > MAX_ATTACHED_CONTEXT_NOTE_BYTES
  ) {
    return null;
  }
  return {
    kind: "diffLineComment",
    diffRef,
    side: value.side,
    startLine,
    endLine,
    note,
  };
}

export function validateAttachedContexts(
  values: readonly DiffLineCommentContext[],
): readonly DiffLineCommentContext[] {
  if (values.length > MAX_ATTACHED_CONTEXTS_PER_TURN) {
    throw new Error(
      `Attached context entries exceed ${MAX_ATTACHED_CONTEXTS_PER_TURN}`,
    );
  }
  return values.map((value) => {
    const parsed = parseDiffLineCommentContext(value);
    if (!parsed) throw new Error("Attached diff context is invalid");
    return parsed;
  });
}

export function diffLineCommentContext(
  diffRef: AgentDiffRef,
  side: DiffLineSide,
  startLine: number,
  endLine: number,
  note: string,
): DiffLineCommentContext {
  const parsed = parseDiffLineCommentContext({
    kind: "diffLineComment",
    diffRef,
    side,
    startLine: String(startLine),
    endLine: String(endLine),
    note,
  });
  if (!parsed) throw new Error("Attached diff context is invalid");
  return parsed;
}

export function attachedContextKey(context: DiffLineCommentContext): string {
  const ref = context.diffRef;
  return JSON.stringify([
    ref.projectId,
    ref.chatId,
    ref.sessionId,
    ref.runtimeEpoch,
    ref.turnId,
    ref.itemId,
    ref.diffOrdinal,
    ref.snapshotRevision,
    ref.rootId,
    ref.relativePath,
    ref.contentDigest,
    context.side,
    context.startLine,
    context.endLine,
    context.note,
  ]);
}

export function attachedContextsEqual(
  left: readonly DiffLineCommentContext[],
  right: readonly DiffLineCommentContext[],
): boolean {
  return (
    left.length === right.length &&
    left.every(
      (context, index) =>
        attachedContextKey(context) === attachedContextKey(right[index]),
    )
  );
}
