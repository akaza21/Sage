/** Native-only contract. Composer chips and browser adapters are separate work. */
export interface PromptFileOwner {
  projectId: string;
  chatId: string;
  projectGeneration: number;
}
export interface StagedPromptFiles {
  revision: string;
  files: { id: string; name: string; bytes: number }[];
}
/** 16 files, 64 KiB/file, 96 KiB/batch; replacement is revision checked. */
export interface PromptAttachmentsPort {
  stagePromptFiles(request: {
    owner: PromptFileOwner;
    paths: string[];
    expectedRevision: string | null;
  }): Promise<StagedPromptFiles>;
  listPromptFiles(owner: PromptFileOwner): Promise<StagedPromptFiles | null>;
  removePromptFile(request: {
    owner: PromptFileOwner;
    revision: string;
    id: string;
  }): Promise<StagedPromptFiles | null>;
}
