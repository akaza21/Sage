import type { RefObject } from "react";
import { IconButton } from "../../ui/IconButton";
import { Paperclip } from "../../ui/leanIcons";
import type * as AttachChunk from "./ComposerAttach";
import type * as MentionChunk from "./ComposerMentionController";
import { lazyParts } from "./lazyParts";
import type { useComposerFileContext } from "./useComposerFiles";
import type { ImageAction } from "./useComposerImageInput";

type FileContext = ReturnType<typeof useComposerFileContext>;

// The first-Chat path carries only these mounts and the attach button.
const MentionPart = lazyParts<typeof MentionChunk>(
  () => import("./ComposerMentionController"),
);
const AttachPart = lazyParts<typeof AttachChunk>(
  () => import("./ComposerAttach"),
);

export function ComposerAttachButton({
  files,
  triggerRef,
  fieldId,
  disabled,
}: {
  files: FileContext;
  triggerRef: RefObject<HTMLButtonElement | null>;
  fieldId: string;
  disabled: boolean;
}) {
  return (
    <IconButton
      ref={triggerRef}
      id={`${fieldId}-attach`}
      className="composer__plus"
      aria-label="Attach files"
      disabled={disabled}
      onClick={files.pressAttach}
    >
      <Paperclip size={15} aria-hidden="true" />
    </IconButton>
  );
}

export function ComposerFileParts({
  files,
  triggerRef,
  chatId,
  fieldRef,
  fieldId,
  text,
  imageAction,
  onPickFiles,
  hidden,
  disabled,
}: {
  files: FileContext;
  triggerRef: RefObject<HTMLButtonElement | null>;
  chatId: string;
  fieldRef: RefObject<HTMLTextAreaElement | null>;
  fieldId: string;
  text: string;
  imageAction?: ImageAction;
  onPickFiles: () => void;
  hidden: boolean;
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
      {/* Only the button and its menu: the staged batch and its chips
          belong to the paste part, so this loads with the first press. */}
      {chatId && files.attachPresses > 0 ? (
        <AttachPart
          part="ComposerAttach"
          chatId={chatId}
          presses={files.attachPresses}
          triggerRef={triggerRef}
          imageAction={imageAction}
          onPick={onPickFiles}
        />
      ) : null}
    </>
  );
}
