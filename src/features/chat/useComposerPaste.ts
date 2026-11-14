import { useState, type ChangeEvent } from "react";

import type { StagedSummary } from "./pastedTextTypes";
import { useComposerPasteSignal } from "./useComposerPasteSignal";

export type ComposerPasteWiring = ReturnType<typeof useComposerPaste>;

export function useComposerPaste(
  nativeAcp: boolean,
  activeTurn: boolean,
  disabledReasonId: string,
  onTextChange: (text: string) => void,
) {
  const signal = useComposerPasteSignal();
  const [staged, setStaged] = useState<StagedSummary>();
  const armed = nativeAcp || signal.seq > 0;
  const noticeId = `${disabledReasonId}-paste`;
  return {
    nativeAcp,
    activeTurn,
    signal,
    setStaged,
    armed,
    noticeId,
    pickFiles: signal.holdPick,
    revision: staged?.revision,
    describedBy: armed && staged ? ` ${noticeId}` : "",
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
