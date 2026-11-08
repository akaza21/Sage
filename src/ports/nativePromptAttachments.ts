import { invokeNative } from "./nativeInvoke";
import type { PromptAttachmentsPort } from "./promptAttachments";

export const nativePromptAttachments: PromptAttachmentsPort = {
  stagePromptFiles: (request) =>
    invokeNative("stage_prompt_files", { request }),
  listPromptFiles: (owner) => invokeNative("list_prompt_files", { owner }),
  removePromptFile: (request) =>
    invokeNative("remove_prompt_file", { request }),
};
