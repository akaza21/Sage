/** What a send needs to know about the chat's staged batch. */
export interface StagedSummary {
  /** The batch revision the next send names; null when nothing is staged. */
  revision: string | null;
  busy: boolean;
  /** False until the batch has been read back from native state. */
  settled: boolean;
}

/** A paste over the composer cap, held for the lazy paste part. */
export interface HeldPaste {
  before: string;
  value: string;
  after: string;
}

/**
 * What the composer hands the lazy paste part: a load (armed), an edit held
 * back for the cap (its size only, never its text), or a held paste.
 */
export type PasteSignal =
  | { kind: "armed"; seq: 0 }
  | { kind: "edit"; bytes: number; seq: number }
  | { kind: "paste"; seq: number };

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
