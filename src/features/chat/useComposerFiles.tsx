import { useRef, useState, type ComponentProps } from "react";
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
}: Omit<ComponentProps<typeof ComposerFileParts>, "files" | "triggerRef"> & {
  attachable: boolean;
}) {
  const files = useComposerFileContext(parts.text);
  // The attach button, which the attach part's menu anchors on.
  const triggerRef = useRef<HTMLButtonElement | null>(null);
  return {
    parts: (
      <ComposerFileParts files={files} triggerRef={triggerRef} {...parts} />
    ),
    attachButton: attachable ? (
      <ComposerAttachButton
        files={files}
        triggerRef={triggerRef}
        fieldId={parts.fieldId}
        disabled={parts.disabled}
      />
    ) : null,
  };
}
