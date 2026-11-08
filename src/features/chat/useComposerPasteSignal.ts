import { useRef, useState, type ClipboardEvent } from "react";

import { MAX_CHAT_COMPOSER_BYTES } from "../../domain/chatOwnership";
import { utf8ByteLength } from "../../domain/text";
import type { HeldPaste, PasteSignal } from "./pastedTextTypes";

/**
 * The UTF-8 size of text over the composer cap, or 0 when it fits. Text
 * longer than the cap in UTF-16 units is over it in bytes too, so a huge
 * clipboard is never encoded here.
 */
function overCap(text: string): number {
  if (text.length * 3 <= MAX_CHAT_COMPOSER_BYTES) return 0;
  const bytes =
    text.length > MAX_CHAT_COMPOSER_BYTES ? text.length : utf8ByteLength(text);
  return bytes > MAX_CHAT_COMPOSER_BYTES ? bytes : 0;
}

/**
 * The composer's eager half of paste handling: only the synchronous size
 * check. An over-cap paste or edit is held back here and handed to the lazy
 * paste part, which stages or refuses it once loaded, so a paste made before
 * that part loads is staged on its arrival, never dropped.
 */
export function useComposerPasteSignal() {
  const [signal, setSignal] = useState<PasteSignal | null>(null);
  const held = useRef<HeldPaste | null>(null);
  const sequence = useRef(0);
  return {
    signal,
    /** Loads the paste part, which then reads what this chat has staged. */
    arm: () => setSignal((current) => current ?? { kind: "armed", seq: 0 }),
    /** The held paste, handed over once. */
    takePaste() {
      const paste = held.current;
      held.current = null;
      return paste;
    },
    /** True when an edit is over the cap and was held back; never truncated. */
    holdEdit(next: string) {
      const over = overCap(next) > 0;
      if (over)
        setSignal({
          kind: "edit",
          bytes: utf8ByteLength(next),
          seq: ++sequence.current,
        });
      return over;
    },
    /** Runs before the form's own paste handling; image files go on to it. */
    onPaste(event: ClipboardEvent<HTMLTextAreaElement>) {
      const data = event.clipboardData;
      if (data.files.length) return;
      const {
        value: draft,
        selectionStart,
        selectionEnd,
      } = event.currentTarget;
      const value = data.getData("text/plain");
      const before = draft.slice(0, selectionStart);
      const after = draft.slice(selectionEnd);
      if (!overCap(before + value + after)) return;
      // Decided synchronously: the browser never inserts an over-cap paste.
      event.preventDefault();
      held.current = { before, value, after };
      setSignal({ kind: "paste", seq: ++sequence.current });
    },
  };
}
