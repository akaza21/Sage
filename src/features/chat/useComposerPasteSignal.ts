import { useRef, useState, type ClipboardEvent } from "react";

import { MAX_CHAT_COMPOSER_BYTES } from "../../domain/chatOwnership";
import { utf8ByteLength } from "../../domain/text";
import {
  PROMPT_ATTACHMENT_COUNT,
  PROMPT_ATTACHMENT_TOTAL_BYTES,
} from "../../ports/promptAttachments";
import type { HeldPaste } from "./pastedTextTypes";

/**
 * A lower bound on the UTF-8 size of text over the composer cap, or 0 when
 * it fits. Text longer than the cap in UTF-16 units is over it in bytes too,
 * so only borderline text is ever encoded, never a huge clipboard.
 */
function overCap(text: string): number {
  if (text.length * 3 <= MAX_CHAT_COMPOSER_BYTES) return 0;
  const bytes =
    text.length > MAX_CHAT_COMPOSER_BYTES ? text.length : utf8ByteLength(text);
  return bytes > MAX_CHAT_COMPOSER_BYTES ? bytes : 0;
}

/**
 * The composer's eager half of paste handling: only the synchronous size
 * check. Over-cap pastes and edits are held back here and handed to the
 * lazy paste part, which stages or refuses each once loaded, so no paste
 * made before that part loads is dropped.
 */
export function useComposerPasteSignal() {
  // One more for each input held back.
  const [seq, setSeq] = useState(0);
  const held = useRef<HeldPaste[]>([]);
  const hold = (input: HeldPaste) => {
    held.current.push(input);
    setSeq((last) => last + 1);
  };
  return {
    seq,
    /** Held input, in order; the paste part drains it. */
    held,
    /**
     * Nonzero (a lower bound on its size) when an edit is over the cap and
     * was held back; never truncated.
     */
    holdEdit(next: string) {
      const bytes = overCap(next);
      if (bytes) hold({ edit: bytes });
      return bytes;
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
      // Only the size of a paste that can never attach is kept: one over
      // the batch limit, or one past the count limit (a bound on held input).
      hold(
        value.length > PROMPT_ATTACHMENT_TOTAL_BYTES ||
          held.current.length >= PROMPT_ATTACHMENT_COUNT
          ? { bytes: value.length }
          : { before, value, after },
      );
    },
  };
}
