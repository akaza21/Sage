import { useState, type ComponentProps } from "react";
import { ComposerAttachButton, ComposerFileParts } from "./ComposerFileContext";

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
  return {
    parts: <ComposerFileParts files={files} {...parts} />,
    attachButton: attachable ? (
      <ComposerAttachButton
        files={files}
        fieldId={parts.fieldId}
        disabled={parts.disabled}
      />
    ) : null,
  };
}
