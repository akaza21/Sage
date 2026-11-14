import type { PromptPick } from "../../ports/promptAttachments";

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
 * Input the composer held back for the lazy paste part, oldest first. Only
 * a paste that may still attach keeps its text; the notices keep at most a
 * size, a lower bound in UTF-8 bytes.
 */
export type HeldInput =
  | { kind: "paste"; paste: HeldPaste }
  /** A pick from the attach menu: native's dialog adds to the batch. */
  | { kind: "pick"; pick: PromptPick }
  /** A paste over the attachment limit, refused when it was made. */
  | { kind: "overLimit"; bytes: number }
  /** A paste made while another was still held, refused when it was made. */
  | { kind: "pending" }
  /** An edit over the composer cap, held back from the draft. */
  | { kind: "edit"; bytes: number };

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
