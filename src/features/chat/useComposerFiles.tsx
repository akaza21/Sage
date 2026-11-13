import { useState, type ComponentProps } from "react";
import { ComposerAttachButton, ComposerFileParts } from "./ComposerFileContext";

/**
 * The composer's file context: the `@` picker arms with the draft's first
 * `@`, the attach menu and picker with the first press of its button. Presses are counted so the loaded attach part takes each one
 * once.
 */
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

/**
 * The composer's file context, ready to place: the `@` picker and attach
 * menu beside the field, and the attach button while a binding is usable.
 * Picked files join the staged batch the paste part owns and shows.
 */
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
