import { useEffect, useState, type RefObject } from "react";
import { useWorkbenchStore } from "../../app/store";
import { isTranscriptQuote } from "../../domain/transcriptQuote";
import { TranscriptPart } from "./lazyParts";

/** Lazy interactions load on selection, pending annotation or open side chat. */
export { TranscriptPart };

/** Arm once on selected pointer-up/Shift+Arrow key-up, answer context-menu key, or pending annotation (1=pointer, 2=keyboard). Chunk validates quotes; detach listeners on arm/unmount. */
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
        // Only Shift+Arrow arms keyboard selection, never Ctrl+C after pointer selection.
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
