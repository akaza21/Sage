export interface StagedSummary {
  revision: string | null;
  busy: boolean;
  settled: boolean;
}

export interface HeldPaste {
  before: string;
  value: string;
  after: string;
}

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
  text: string | null;
  lines: number | null;
}

export interface PastedTextState {
  revision: string | null;
  items: readonly StagedPromptItem[];
  busy: boolean;
}
