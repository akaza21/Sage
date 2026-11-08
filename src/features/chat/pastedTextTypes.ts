/** What a send needs to know about the chat's staged batch. */
export interface StagedSummary {
  /** The batch revision the next send names; null when nothing is staged. */
  revision: string | null;
  busy: boolean;
  /** False until the batch has been read back from native state. */
  settled: boolean;
}

/** A paste over the composer cap, held for the lazy paste part. */
export interface HeldText {
  before: string;
  value: string;
  after: string;
}

/**
 * Input held for the lazy paste part, in order: a paste's text; only the
 * size (a lower bound in bytes) of a paste that can never attach, being over
 * the batch limit or past the count limit; or only the size of an edit held
 * back for the composer cap, never its text.
 */
export type HeldPaste = HeldText | { bytes: number } | { edit: number };

/** One staged resource. Only a paste made here keeps its text for preview. */
export interface StagedPromptItem {
  id: string;
  name: string;
  bytes: number;
  /** The pasted text, bounded by the native batch budget; null for files. */
  text: string | null;
  lines: number | null;
}

export interface PastedTextState {
  revision: string | null;
  items: readonly StagedPromptItem[];
  busy: boolean;
}
