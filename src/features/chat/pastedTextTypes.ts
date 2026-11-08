/** What a send needs to know about the chat's staged batch. */
export interface StagedSummary {
  /** The batch revision the next send names; null when nothing is staged. */
  revision: string | null;
  busy: boolean;
}

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
