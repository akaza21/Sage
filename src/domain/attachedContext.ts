import type { AgentDiffRef } from "./agentDiff";
import { decimalU64Counter, isRecord } from "./guards";
import { utf8ByteLength } from "./text";
import {
  parseTranscriptQuoteContext,
  type TranscriptQuoteContext,
} from "./transcriptQuote";
import {
  parseBrowserElementContext,
  parseBrowserPageContext,
  unsafeDisplayCharacter,
  type BrowserElementContext,
  type BrowserPageContext,
} from "./browserContext";

export {
  MAX_BROWSER_PAGE_TEXT_BYTES,
  parseBrowserElementContext,
  parseBrowserPageContext,
  type BrowserElementContext,
  type BrowserPageContext,
} from "./browserContext";

/** One turn may carry only a small review set, not an unbounded file annotation log. */
export const MAX_ATTACHED_CONTEXTS_PER_TURN = 8;
/** Matches a quarter of the composer budget and keeps eight notes within 32 KiB. */
export const MAX_ATTACHED_CONTEXT_NOTE_BYTES = 4 * 1024;
/*
 * Image mirrors of `src-tauri/src/acp/limits/images.rs`, checked there by a
 * parity test that reads this file.
 */
/** Mirrors `STORED_IMAGE_BYTES`: a stored image, for any harness. */
export const MAX_OUTGOING_IMAGE_BYTES = 4 * 1024 * 1024;
/** Mirrors `PROMPT_IMAGE_COUNT`. */
export const MAX_PROMPT_IMAGES = 4;
/**
 * Mirrors `IMAGE_UPLOAD_BYTES`: an attached file before Sage normalises it.
 * A send then checks the images against the prompt bounds and names any that
 * still exceed them.
 */
export const MAX_IMAGE_UPLOAD_BYTES = 10 * 1024 * 1024;

/** `bytes` as KiB or MiB, for messages built from the mirrored limits. */
export function formatImageBytes(bytes: number): string {
  return bytes % (1024 * 1024) === 0
    ? `${bytes / (1024 * 1024)} MiB`
    : `${Math.floor(bytes / 1024)} KiB`;
}

export const imageUploadLimitMessage = `Choose PNG or JPEG images no larger than ${formatImageBytes(MAX_IMAGE_UPLOAD_BYTES)} each.`;

export type DiffLineSide = "old" | "new";

export interface DiffLineCommentContext {
  kind: "diffLineComment";
  diffRef: AgentDiffRef;
  side: DiffLineSide;
  startLine: string;
  endLine: string;
  note: string;
}

export interface ImageAttachedContext {
  kind: "image";
  artifactId: string;
  mediaType: "image/png" | "image/jpeg";
  width: number;
  height: number;
  byteLength: number;
  sha256: string;
  label: string;
}

export type AttachedContext =
  | DiffLineCommentContext
  | BrowserPageContext
  | BrowserElementContext
  | ImageAttachedContext
  | TranscriptQuoteContext;

export function parseImageAttachedContext(
  value: unknown,
): ImageAttachedContext | null {
  if (!isRecord(value) || value.kind !== "image") return null;
  const keys = Object.keys(value).sort();
  if (
    keys.join(",") !==
    "artifactId,byteLength,height,kind,label,mediaType,sha256,width"
  )
    return null;
  const { artifactId, mediaType, width, height, byteLength, sha256, label } =
    value;
  if (
    typeof artifactId !== "string" ||
    !/^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/.test(
      artifactId,
    ) ||
    (mediaType !== "image/png" && mediaType !== "image/jpeg") ||
    typeof width !== "number" ||
    !Number.isInteger(width) ||
    width < 1 ||
    width > 2048 ||
    typeof height !== "number" ||
    !Number.isInteger(height) ||
    height < 1 ||
    height > 2048 ||
    typeof byteLength !== "number" ||
    !Number.isInteger(byteLength) ||
    byteLength < 1 ||
    byteLength > MAX_OUTGOING_IMAGE_BYTES ||
    typeof sha256 !== "string" ||
    !/^[0-9a-f]{64}$/.test(sha256) ||
    typeof label !== "string" ||
    !label ||
    utf8ByteLength(label) > 80 ||
    [...label].some(unsafeDisplayCharacter)
  )
    return null;
  return {
    kind: "image",
    artifactId,
    mediaType,
    width,
    height,
    byteLength,
    sha256,
    label,
  };
}

export const isBrowserElement = (
  context: AttachedContext,
): context is BrowserElementContext => context.kind === "browserElement";

export function parseAttachedContext(value: unknown): AttachedContext | null {
  return (
    parseTranscriptQuoteContext(value) ??
    parseBrowserPageContext(value) ??
    parseBrowserElementContext(value) ??
    parseDiffLineCommentContext(value) ??
    parseImageAttachedContext(value)
  );
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

export function queuedAttachmentCue(context: AttachedContext): {
  kind: "file" | "page" | "image";
  label: string;
} {
  if (context.kind === "transcriptQuote")
    return { kind: "file", label: "Annotation" };
  if (context.kind === "browserElement")
    return { kind: "page", label: `Element ${context.ordinal}` };
  if (context.kind === "browserPage") {
    const parsed = parseBrowserPageContext(context);
    return {
      kind: "page",
      label: parsed ? new URL(parsed.url).host.slice(0, 64) : "Page",
    };
  }
  if (context.kind === "image") {
    return { kind: "image", label: context.label };
  }
  const parsed = parseDiffLineCommentContext(context);
  return {
    kind: "file",
    label: parsed
      ? (parsed.diffRef.relativePath.split("/").at(-1) ?? "File").slice(0, 64)
      : "File",
  };
}

export function validateAttachedContexts(
  values: readonly AttachedContext[],
): readonly AttachedContext[] {
  if (values.length > MAX_ATTACHED_CONTEXTS_PER_TURN) {
    throw new Error(
      `Attached context entries exceed ${MAX_ATTACHED_CONTEXTS_PER_TURN}`,
    );
  }
  const parsed = values.map((value) => {
    const context = parseAttachedContext(value);
    if (!context) throw new Error("Attached context is invalid");
    return context;
  });
  // Contract browser-element-context-v1 §2.1: element numbers are unique.
  const numbers = parsed.filter(isBrowserElement).map(({ ordinal }) => ordinal);
  if (new Set(numbers).size < numbers.length) throw new Error(REPEATED_ELEMENT);
  return parsed;
}

export const REPEATED_ELEMENT = "Two page elements share a number; remove one.";

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

export function attachedContextKey(context: AttachedContext): string {
  if (context.kind === "transcriptQuote")
    return `quote:${context.sourceItemId}:${context.startOffset}-${context.endOffset}`;
  if (context.kind === "browserPage") {
    return JSON.stringify([
      context.kind,
      context.tabId,
      context.url,
      context.title,
      context.text,
    ]);
  }
  if (context.kind === "image") {
    return JSON.stringify([context.kind, context.artifactId, context.sha256]);
  }
  if (context.kind === "browserElement")
    return `element:${context.tabId}:${context.ordinal}`;
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
  left: readonly AttachedContext[],
  right: readonly AttachedContext[],
): boolean {
  return (
    left.length === right.length &&
    left.every(
      (context, index) =>
        attachedContextKey(context) === attachedContextKey(right[index]),
    )
  );
}

export function attachContextToDraft(
  contexts: readonly AttachedContext[],
  context: AttachedContext,
): readonly AttachedContext[] {
  const key = attachedContextKey(context);
  const index = contexts.findIndex(
    (candidate) => attachedContextKey(candidate) === key,
  );
  if (index >= 0 && context.kind !== "transcriptQuote") return contexts;
  const next = [...contexts];
  if (index < 0) next.push(context);
  else next[index] = context;
  return validateAttachedContexts(next);
}
