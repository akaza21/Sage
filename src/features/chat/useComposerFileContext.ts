import { useState } from "react";

/**
 * The composer's file context: the `@` picker arms with the draft's first
 * `@`, the attach flow with the first press of its button. Presses are
 * counted so the loaded attach part takes each one once.
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
