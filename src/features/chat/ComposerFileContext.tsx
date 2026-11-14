import type { RefObject } from "react";
import type { PromptPick } from "../../ports/promptAttachments";
import type * as AttachChunk from "./ComposerAttach";
import type { ComposerImageAction } from "./ComposerAttach";
import type * as MentionChunk from "./ComposerMentionController";
import { lazyParts } from "./lazyParts";
import type { useComposerFileContext } from "./useComposerFiles";

type FileContext = ReturnType<typeof useComposerFileContext>;

// The first-Chat path carries only these mounts.
const MentionPart = lazyParts<typeof MentionChunk>(
  () => import("./ComposerMentionController"),
);
const AttachPart = lazyParts<typeof AttachChunk>(
  () => import("./ComposerAttach"),
);

export function ComposerFileParts({
  files,
  chatId,
  fieldRef,
  fieldId,
  attachAnchorId,
  text,
  imageAction,
  onPickFiles,
  hidden,
  disabled,
}: {
  files: FileContext;
  chatId: string;
  fieldRef: RefObject<HTMLTextAreaElement | null>;
  fieldId: string;
  /** The control the attach menu anchors on. */
  attachAnchorId: string;
  text: string;
  imageAction?: ComposerImageAction;
  /** Hands a pick to the chat's staged batch, which the paste part owns. */
  onPickFiles: (pick: PromptPick) => void;
  hidden: boolean;
  /** The field cannot be edited. */
  disabled: boolean;
}) {
  return (
    <>
      {files.mentionArmed && !hidden ? (
        <MentionPart
          part="ComposerMentionController"
          fieldRef={fieldRef}
          disabled={disabled}
          fieldId={fieldId}
          text={text}
        />
      ) : null}
      {/* Only the attach menu: the staged batch and its chips belong to
          the paste part, so this loads with the first press. */}
      {chatId && files.attachPresses > 0 ? (
        <AttachPart
          part="ComposerAttach"
          chatId={chatId}
          presses={files.attachPresses}
          triggerId={attachAnchorId}
          imageAction={imageAction}
          onPick={onPickFiles}
        />
      ) : null}
    </>
  );
}
