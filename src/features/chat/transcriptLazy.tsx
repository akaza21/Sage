import { useEffect, useState, type RefObject } from "react";
import { useWorkbenchStore } from "../../app/store";
import { isTranscriptQuote } from "../../domain/transcriptQuote";
import { TranscriptPart } from "./lazyParts";

/**
 * Core entry points for transcript selection, annotations and side chats.
 * Everything else lives in one lazy chunk (TranscriptInteractions.tsx) that
 * loads on the first selection, a pending annotation or an open side chat.
 */
export { TranscriptPart };

/**
 * Tiny core trigger. Pointer-up or a Shift+Arrow key-up with a live
 * selection, or the menu key (Shift+F10 / ContextMenu) on an answer, arms
 * the lazy chunk once (1 = pointer, 2 = keyboard); a
 * pending annotation arms it straight away. The chunk decides whether the
 * selection is quotable. Listeners detach once armed or on unmount.
 */
export function TranscriptInteractionsGate({
  chatId,
  focused,
  contentRef,
}: {
  chatId: string;
  focused: boolean;
  contentRef: RefObject<HTMLElement | null>;
}) {
  const [armed, setArmed] = useState<0 | 1 | 2>(0);
  const quoted = useWorkbenchStore((state) =>
    Boolean(state.chatUiById[chatId]?.attachedContext?.some(isTranscriptQuote)),
  );
  useEffect(() => {
    const content = contentRef.current;
    if (armed || !content) return;
    const check = (event: Event) => {
      const { key, shiftKey, target } = event as KeyboardEvent;
      if (event.type === "pointerup") {
        if (getSelection()?.isCollapsed === false) setArmed(1);
      } else if (event.type === "keyup") {
        // Only a Shift+Arrow selection is a keyboard selection; Ctrl+C and
        // other keys after a mouse selection never arm keyboard mode.
        if (
          shiftKey &&
          key?.startsWith("Arrow") &&
          getSelection()?.isCollapsed === false
        )
          setArmed(2);
      } else if (
        (key === "ContextMenu" || (key === "F10" && shiftKey)) &&
        (target as Element).closest?.("[data-selectable-answer]")
      ) {
        event.preventDefault();
        setArmed(2);
      }
    };
    const types = ["pointerup", "keyup", "keydown"];
    for (const type of types) content.addEventListener(type, check);
    return () => {
      for (const type of types) content.removeEventListener(type, check);
    };
  }, [armed, contentRef]);
  return armed || quoted ? (
    <TranscriptPart
      part="TranscriptInteractions"
      chatId={chatId}
      focused={focused}
      contentRef={contentRef}
      initialKeyboard={armed === 2}
    />
  ) : null;
}
