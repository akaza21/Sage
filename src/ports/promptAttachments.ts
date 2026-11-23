/** Mirrors prompt_files.rs: a batch holds at most this many resources. */
export const PROMPT_ATTACHMENT_COUNT = 16;
/** Mirrors prompt_files.rs TOTAL_BYTES: one batch, pasted text included. */
export const PROMPT_ATTACHMENT_TOTAL_BYTES = 96 * 1024;

/** Native-only contract; staged bytes stay in Rust until a send consumes them. */
export interface PromptChatOwner {
  projectId: string;
  chatId: string;
  projectGeneration: number;
}
/** The new-chat draft, before its chat exists, by a UUID it makes for itself. */
export interface PromptDraftOwner {
  draftToken: string;
}
/** A batch's owner: a chat, or the draft whose first send hands it over. */
export type PromptFileOwner = PromptChatOwner | PromptDraftOwner;
export interface StagedPromptFiles {
  revision: string;
  files: { id: string; name: string; bytes: number }[];
  added?: string;
}
/** What native's dialog picks: files, or one folder whose files it walks. */
export type PromptPick = "files" | "folder";
/** 16 files, 64 KB/file, 96 KB/batch; replacement is revision checked. */
export interface PromptAttachmentsPort {
  /** Opens the native picker; the WebView never supplies paths. Null when cancelled. */
  stagePromptFiles(request: {
    owner: PromptFileOwner;
    pick: PromptPick;
    expectedRevision: string | null;
  }): Promise<StagedPromptFiles | null>;
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
  /** Moves a batch whole between a draft and a chat, or nothing. */
  handOffPromptFiles(request: {
    from: PromptFileOwner;
    to: PromptFileOwner;
    revision: string;
  }): Promise<StagedPromptFiles>;
  releaseDraftPromptFiles(draft: PromptDraftOwner): Promise<void>;
}
