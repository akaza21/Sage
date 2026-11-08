import type { RefObject } from "react";
import { IconButton } from "../../ui/IconButton";
import { Paperclip } from "../../ui/leanIcons";
import type * as AttachChunk from "./ComposerAttach";
import type * as MentionChunk from "./ComposerMentionController";
import { lazyParts } from "./lazyParts";
import type { useComposerFileContext } from "./useComposerFileContext";

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
  fieldId,
  disabled,
}: {
  files: FileContext;
  fieldId: string;
  disabled: boolean;
}) {
  return (
    <IconButton
      // Anchors the attach menu.
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
  onTextChange,
  imageAction,
  hidden,
  disabled,
  busy,
}: {
  files: FileContext;
  chatId: string;
  fieldRef: RefObject<HTMLTextAreaElement | null>;
  fieldId: string;
  text: string;
  onTextChange: (text: string) => void;
  imageAction?: { disabled: boolean; onChoose: () => void };
  hidden: boolean;
  /** The field cannot be edited. */
  disabled: boolean;
  /** A send is in flight and may consume the staged files. */
  busy: boolean;
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
          onTextChange={onTextChange}
        />
      ) : null}
      {files.attachPresses > 0 && chatId ? (
        <AttachPart
          part="ComposerAttach"
          chatId={chatId}
          presses={files.attachPresses}
          triggerId={`${fieldId}-attach`}
          imageAction={imageAction}
          disabled={busy}
        />
      ) : null}
    </>
  );
}
