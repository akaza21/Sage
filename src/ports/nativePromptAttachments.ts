import { invokeNative } from "./nativeInvoke";
import type { PromptAttachmentsPort } from "./promptAttachments";

export const nativePromptAttachments: PromptAttachmentsPort = {
  stagePromptFiles: (request) =>
    invokeNative("stage_prompt_files", { request }),
  stagePromptText: (request) => invokeNative("stage_prompt_text", { request }),
  listPromptFiles: (owner) => invokeNative("list_prompt_files", { owner }),
  removePromptFile: (request) =>
    invokeNative("remove_prompt_file", { request }),
  handOffPromptFiles: (request) =>
    invokeNative("hand_off_prompt_files", { request }),
  releaseDraftPromptFiles: (draft) =>
    invokeNative("release_draft_prompt_files", { draft }),
};
