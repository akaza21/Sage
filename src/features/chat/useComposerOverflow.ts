import { useState, type ChangeEvent, type ClipboardEvent } from "react";

import { MAX_CHAT_COMPOSER_BYTES } from "../../domain/chatOwnership";
import { utf8ByteLength } from "../../domain/text";
import {
  loadPasteFlow,
  type PastedTextAttachments,
} from "./usePastedTextAttachments";

type Flow = Awaited<ReturnType<typeof loadPasteFlow>>;

/**
 * Keeps the composer within its byte cap without silently losing input: a
 * longer paste becomes a pasted-text attachment or is refused with a message,
 * and an edit past the cap is held back with a message. Both are announced.
 */
export function useComposerOverflow(
  text: string,
  onTextChange: (text: string) => void,
  pasted: PastedTextAttachments | undefined,
) {
  const [status, setStatus] = useState<string | null>(null);
  const flow = (run: (flow: Flow) => unknown) => void loadPasteFlow().then(run);
  return {
    status,
    onChange(event: ChangeEvent<HTMLTextAreaElement>) {
      const next = event.target.value;
      const bytes = utf8ByteLength(next);
      // Shrinking is always allowed, even for a draft already over the cap.
      if (bytes > MAX_CHAT_COMPOSER_BYTES && bytes > utf8ByteLength(text)) {
        flow((m) => setStatus(m.typedOverflowMessage(bytes)));
        return;
      }
      setStatus(null);
      onTextChange(next);
    },
    onPaste(event: ClipboardEvent<HTMLTextAreaElement>) {
      // Image files belong to the form's image input.
      if (event.clipboardData.files.length) return;
      const value = event.clipboardData.getData("text/plain");
      const area = event.currentTarget;
      const before = area.value.slice(0, area.selectionStart);
      const after = area.value.slice(area.selectionEnd);
      if (utf8ByteLength(before + value + after) <= MAX_CHAT_COMPOSER_BYTES)
        return;
      // Decided synchronously: the browser never inserts an over-cap paste.
      event.preventDefault();
      flow((m) => m.pasteOver(pasted, before, value, after, setStatus));
    },
    // Chips render only while a batch exists, so `pasted` is present.
    removeAttachment: (id: string, label: string) =>
      flow((m) => m.removeStaged(pasted!, id, label, setStatus)),
  };
}
