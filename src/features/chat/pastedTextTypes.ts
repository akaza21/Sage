export interface StagedSummary {
  /** The batch revision the next send names; null when nothing is staged. */
  revision: string | null;
  busy: boolean;
  settled: boolean;
}

export interface HeldPaste {
  before: string;
  value: string;
  after: string;
}

// Notices keep at most a size, a lower bound in UTF-8 bytes.
export type HeldInput =
  | { kind: "paste"; paste: HeldPaste }
  | { kind: "pick" }
  | { kind: "overLimit"; bytes: number }
  | { kind: "pending" }
  | { kind: "edit"; bytes: number };

export interface StagedPromptItem {
  id: string;
  name: string;
  bytes: number;
  /** The pasted text; null for files. */
  text: string | null;
  lines: number | null;
}

export interface PastedTextState {
  revision: string | null;
  items: readonly StagedPromptItem[];
  busy: boolean;
}
