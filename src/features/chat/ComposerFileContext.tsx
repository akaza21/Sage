import { useMemo, type RefObject } from "react";
import type { PromptPick } from "../../ports/promptAttachments";
import type * as AttachChunk from "./ComposerAttach";
import type * as MentionChunk from "./ComposerMentionController";
import { lazyParts } from "./lazyParts";
import type { PickedImages } from "./pastedTextTypes";
import type { useComposerFileContext } from "./useComposerFiles";
import type { ImageAction } from "./useComposerImageInput";

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
  attachAnchorId: string;
  text: string;
  imageAction?: ImageAction;
  /** Hands a pick to the chat's staged batch, which the paste part owns;
   * with `images`, picked images go as images. */
  onPickFiles: (pick: PromptPick, images?: PickedImages) => void;
  hidden: boolean;
  disabled: boolean;
}) {
  // The anchor belongs to the toolbar: it is looked up whenever the menu
  // reads it, so the menu is placed from its first commit.
  const anchor = useMemo(
    () => ({
      get current() {
        return document.getElementById(attachAnchorId);
      },
    }),
    [attachAnchorId],
  );
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
      {files.attachPresses > 0 ? (
        <AttachPart
          part="ComposerAttach"
          chatId={chatId}
          presses={files.attachPresses}
          triggerRef={anchor}
          imageAction={imageAction}
          onPick={onPickFiles}
        />
      ) : null}
    </>
  );
}
