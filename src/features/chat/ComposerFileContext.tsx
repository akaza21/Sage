import type { RefObject } from "react";
import { IconButton } from "../../ui/IconButton";
import { Paperclip } from "../../ui/leanIcons";
import type * as AttachChunk from "./ComposerAttach";
import type * as MentionChunk from "./ComposerMentionController";
import { lazyParts } from "./lazyParts";
import type { useComposerFileContext } from "./useComposerFiles";

type FileContext = ReturnType<typeof useComposerFileContext>;

const MentionPart = lazyParts<typeof MentionChunk>(
  () => import("./ComposerMentionController"),
);
const AttachPart = lazyParts<typeof AttachChunk>(
  () => import("./ComposerAttach"),
);

export function ComposerAttachButton({
  files,
  fieldId,
  disabled,
}: {
  files: FileContext;
  fieldId: string;
  disabled: boolean;
}) {
  return (
    <IconButton
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
  chatId: string;
  fieldRef: RefObject<HTMLTextAreaElement | null>;
  fieldId: string;
  text: string;
  imageAction?: { disabled: boolean; onChoose: () => void };
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
      {chatId && files.attachPresses > 0 ? (
        <AttachPart
          part="ComposerAttach"
          chatId={chatId}
          presses={files.attachPresses}
          triggerId={`${fieldId}-attach`}
          imageAction={imageAction}
          onPick={onPickFiles}
        />
      ) : null}
    </>
  );
}
