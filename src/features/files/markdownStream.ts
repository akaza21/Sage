import { truncateUtf8 } from "../../domain/text";
import { MAX_MARKDOWN_PREVIEW_SOURCE_BYTES } from "./markdownLimits";
import {
  renderMarkdownPreview,
  type MarkdownPreviewDocument,
} from "./markdownPreview";
import { markdownBlockOffsets, retainMarkdownPrefix } from "./markdownTokens";

export interface MarkdownStream {
  source: string;
  document: MarkdownPreviewDocument;
}

/** Owned by a mounted answer, never a global message/transcript cache. */
export function renderMarkdownStream(
  source: string,
  previous?: MarkdownStream,
): MarkdownStream {
  const windowed = truncateUtf8(source, MAX_MARKDOWN_PREVIEW_SOURCE_BYTES);
  const normalized = windowed.replace(/\r\n?/gu, "\n");
  const offsets = previous ? markdownBlockOffsets(previous.document) : [];
  // Keep the final two top-level blocks open: setext headings, list/quote
  // continuation and a newly completed table delimiter can change their kind.
  const offset = offsets[Math.max(0, offsets.length - 2)] ?? 0;
  const count = offsets.findIndex((value) => value === offset);
  const prefix =
    previous && normalized.startsWith(previous.source) && offset > 0
      ? retainMarkdownPrefix(previous.document, count)
      : undefined;
  const document = prefix
    ? renderMarkdownPreview(normalized.slice(offset), prefix, offset)
    : renderMarkdownPreview(normalized);
  document.sourceLimitExceeded = windowed.length < source.length;
  return { source: normalized, document };
}
