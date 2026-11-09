import type { RefObject } from "react";
import { isTauriRuntime } from "../../ports/nativeInvoke";
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
        />
      ) : null}
      {/* In the desktop app it mounts with the chat, so a batch native
          already holds (staged before a reload) shows its chips; elsewhere
          only a press can explain why attaching is unavailable. */}
      {chatId && (files.attachPresses > 0 || isTauriRuntime()) ? (
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
