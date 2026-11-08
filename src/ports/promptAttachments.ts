/** Mirrors prompt_files.rs: a batch holds at most this many resources. */
export const PROMPT_ATTACHMENT_COUNT = 16;
/** Mirrors prompt_files.rs TOTAL_BYTES: one batch, pasted text included. */
export const PROMPT_ATTACHMENT_TOTAL_BYTES = 96 * 1024;

/** Native-only contract; staged bytes stay in Rust until a send consumes them. */
export interface PromptFileOwner {
  projectId: string;
  chatId: string;
  projectGeneration: number;
}
export interface StagedPromptFiles {
  revision: string;
  files: { id: string; name: string; bytes: number }[];
  /** The resource a paste just added. */
  added?: string;
}
/** 16 files, 64 KiB/file, 96 KiB/batch; replacement is revision checked. */
export interface PromptAttachmentsPort {
  /** Opens the native picker; the WebView never supplies paths. Null when cancelled. */
  stagePromptFiles(request: {
    owner: PromptFileOwner;
    pick: "files" | "folder";
    expectedRevision: string | null;
  }): Promise<StagedPromptFiles | null>;
  /** Adds pasted composer text to the batch as one text resource. */
  stagePromptText(request: {
    owner: PromptFileOwner;
    expectedRevision: string | null;
    text: string;
  }): Promise<StagedPromptFiles>;
  listPromptFiles(owner: PromptFileOwner): Promise<StagedPromptFiles | null>;
  removePromptFile(request: {
    owner: PromptFileOwner;
    revision: string;
    id: string;
  }): Promise<StagedPromptFiles | null>;
}
