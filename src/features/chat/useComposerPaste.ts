import { useState, type ChangeEvent } from "react";

import type { StagedSummary } from "./pastedTextTypes";
import { useComposerPasteSignal } from "./useComposerPasteSignal";

export type ComposerPasteWiring = ReturnType<typeof useComposerPaste>;

/**
 * The composer's eager paste wiring: the field's paste and edit handlers,
 * whether the paste part is in place, and the batch it reports staged.
 */
export function useComposerPaste(
  nativeAcp: boolean,
  activeTurn: boolean,
  disabledReasonId: string,
  onTextChange: (text: string) => void,
) {
  const signal = useComposerPasteSignal();
  const [staged, setStaged] = useState<StagedSummary>();
  // A native chat loads it at once, so the batch staged before a reload is
  // read back before any send, whether or not the field was focused.
  const armed = nativeAcp || signal.seq > 0;
  // The paste notices the field names, once the paste part is in place.
  const noticeId = `${disabledReasonId}-paste`;
  return {
    nativeAcp,
    activeTurn,
    signal,
    setStaged,
    armed,
    noticeId,
    /** The staged batch a send carries, if any. */
    revision: staged?.revision,
    /** Appended to the field's aria-describedby. */
    describedBy: armed && staged ? ` ${noticeId}` : "",
    // Staged pasted text goes with a sent message, never a queued one, and
    // no send goes before the batch staged earlier has been read back.
    blocksSend: Boolean(
      staged?.busy ||
      (activeTurn && staged?.revision) ||
      (armed && !staged?.settled),
    ),
    field: {
      onPaste: signal.onPaste,
      onChange(event: ChangeEvent<HTMLTextAreaElement>) {
        const next = event.target.value;
        // Held back with a message, never truncated. A draft is never over
        // the cap, so this never blocks a deletion.
        if (!signal.holdEdit(next)) onTextChange(next);
      },
    },
  };
}
