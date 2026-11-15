import { useState, type ComponentProps } from "react";
import { ComposerFileParts } from "./ComposerFileContext";
import { imagesOffered } from "./useComposerImageInput";

export function useComposerFileContext(text: string) {
  const [mentionArmed, setMentionArmed] = useState(false);
  if (!mentionArmed && text.includes("@")) setMentionArmed(true);
  const [attachPresses, setAttachPresses] = useState(0);
  return {
    mentionArmed,
    attachPresses,
    pressAttach: () => setAttachPresses((count) => count + 1),
  };
}

export function useComposerFiles({
  attachable,
  ...parts
}: Omit<ComponentProps<typeof ComposerFileParts>, "files"> & {
  attachable: boolean;
}) {
  const files = useComposerFileContext(parts.text);
  const attach = attachable && !parts.disabled ? files.pressAttach : undefined;
  return {
    parts: <ComposerFileParts files={files} {...parts} />,
    attach,
    /** Images keep the attach menu open where files are refused. */
    refusalChatId:
      attach && !imagesOffered(parts.imageAction) ? parts.chatId : undefined,
  };
}
