import { useCallback, useRef, useState, type ClipboardEvent } from "react";

import { MAX_CHAT_COMPOSER_BYTES } from "../../domain/chatOwnership";
import { utf8ByteLength } from "../../domain/text";
import {
  PROMPT_ATTACHMENT_TOTAL_BYTES,
  type PromptPick,
} from "../../ports/promptAttachments";
import type { HeldInput } from "./pastedTextTypes";

/** UTF-8 takes at most this many bytes per UTF-16 code unit. */
const MAX_UTF8_BYTES_PER_UNIT = 3;

/**
 * A lower bound on the UTF-8 size of text over the composer cap, or 0 when
 * it fits. The cheap bounds come first: text whose upper bound fits is
 * never encoded, and text longer than the cap in UTF-16 units is over it
 * in bytes too, so only borderline text is encoded, never a huge clipboard.
 */
function overCap(text: string): number {
  if (text.length * MAX_UTF8_BYTES_PER_UNIT <= MAX_CHAT_COMPOSER_BYTES)
    return 0;
  if (text.length > MAX_CHAT_COMPOSER_BYTES) return text.length;
  const bytes = utf8ByteLength(text);
  return bytes > MAX_CHAT_COMPOSER_BYTES ? bytes : 0;
}

/** Input held only to say something about it. */
function isNotice(input: HeldInput | undefined): boolean {
  return input !== undefined && input.kind !== "paste" && input.kind !== "pick";
}

/**
 * The composer's eager half of paste handling: only the synchronous size
 * check. Over-cap pastes and edits are held back here, in order, and the
 * lazy paste part takes them all whenever it handles one, so a paste made
 * before that part loads is staged on its arrival, never dropped.
 *
 * What is held stays small: the text of at most one paste, which the part
 * takes as soon as it can; a second long paste made meanwhile is refused.
 * A paste that could never attach keeps only its size. Only the latest
 * notice can show, so a notice replaces one held just before it. A pick
 * from the attach button is held the same way, so the part stages pasted
 * and picked files into one batch, one change at a time.
 */
export function useComposerPasteSignal() {
  // Bumped for each input held back; 0 while there has been none.
  const [seq, setSeq] = useState(0);
  const queue = useRef<HeldInput[]>([]);
  const hold = useCallback((input: HeldInput) => {
    const held = queue.current;
    // A notice replaces only a notice; pastes and picks are never dropped.
    if (isNotice(input) && isNotice(held.at(-1))) held.pop();
    held.push(input);
    setSeq((last) => last + 1);
  }, []);
  return {
    seq,
    /** Hands over everything held, oldest first, once. */
    take: useCallback(() => queue.current.splice(0), []),
    /** Asks for native's dialog, staged in order with held pastes. */
    holdPick: useCallback(
      (pick: PromptPick) => hold({ kind: "pick", pick }),
      [hold],
    ),
    /** True when an edit is over the cap and was held back; never truncated. */
    holdEdit(next: string) {
      const bytes = overCap(next);
      if (bytes) hold({ kind: "edit", bytes });
      return bytes > 0;
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
      // Its length in UTF-16 units is a lower bound on its UTF-8 size.
      if (value.length > PROMPT_ATTACHMENT_TOTAL_BYTES)
        hold({ kind: "overLimit", bytes: value.length });
      else if (queue.current.some((held) => held.kind === "paste"))
        hold({ kind: "pending" });
      else hold({ kind: "paste", paste: { before, value, after } });
    },
  };
}
