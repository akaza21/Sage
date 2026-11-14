import { useCallback, useRef, useState, type ClipboardEvent } from "react";

import { MAX_CHAT_COMPOSER_BYTES } from "../../domain/chatOwnership";
import { utf8ByteLength } from "../../domain/text";
import { PROMPT_ATTACHMENT_TOTAL_BYTES } from "../../ports/promptAttachments";
import type { HeldInput } from "./pastedTextTypes";

const MAX_UTF8_BYTES_PER_UNIT = 3;

function overCap(text: string): number {
  if (text.length * MAX_UTF8_BYTES_PER_UNIT <= MAX_CHAT_COMPOSER_BYTES)
    return 0;
  if (text.length > MAX_CHAT_COMPOSER_BYTES) return text.length;
  const bytes = utf8ByteLength(text);
  return bytes > MAX_CHAT_COMPOSER_BYTES ? bytes : 0;
}

function isNotice(input: HeldInput | undefined): boolean {
  return input !== undefined && input.kind !== "paste" && input.kind !== "pick";
}

export function useComposerPasteSignal() {
  const [seq, setSeq] = useState(0);
  const queue = useRef<HeldInput[]>([]);
  const hold = useCallback((input: HeldInput) => {
    const held = queue.current;
    if (isNotice(input) && isNotice(held.at(-1))) held.pop();
    held.push(input);
    setSeq((last) => last + 1);
  }, []);
  return {
    seq,
    take: useCallback(() => queue.current.splice(0), []),
    holdPick: useCallback(() => hold({ kind: "pick" }), [hold]),
    holdEdit(next: string) {
      const bytes = overCap(next);
      if (bytes) hold({ kind: "edit", bytes });
      return bytes > 0;
    },
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
      if (value.length > PROMPT_ATTACHMENT_TOTAL_BYTES)
        hold({ kind: "overLimit", bytes: value.length });
      else if (queue.current.some((held) => held.kind === "paste"))
        hold({ kind: "pending" });
      else hold({ kind: "paste", paste: { before, value, after } });
    },
  };
}
