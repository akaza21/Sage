import { useCallback, useEffect, useMemo, useRef, useState } from "react";

import { isTauriRuntime } from "../../ports/nativeInvoke";
import {
  pasteAttachmentRefusal,
  typedOverflowMessage,
  type PasteAttachability,
} from "./composerPaste";
import { PastedTextChips } from "./PastedTextChips";
import {
  pasteOver,
  reconcile,
  removeStaged,
  type StagedOwner,
} from "./pastedTextFlow";
import type {
  HeldPaste,
  PastedTextState,
  PasteSignal,
  StagedSummary,
} from "./pastedTextTypes";

const EMPTY: PastedTextState = { revision: null, items: [], busy: false };

/**
 * Lazy: loads with the composer's first focus or over-cap input. Owns the
 * chat's staged pasted text: attaching a paste too long for the composer,
 * refusing one that cannot attach, the announcements, the chips and their
 * preview. The composer keeps only its synchronous size check and hands
 * over what it held back, a paste made before this part loaded included.
 */
export function ComposerPaste({
  chatId,
  session,
  nativeAcp,
  activeTurn,
  signal,
  takePaste,
  text,
  disabled,
  noticeId,
  reasonId,
  onStaged,
}: {
  chatId: string | undefined;
  session: PasteAttachability["session"];
  nativeAcp: boolean;
  activeTurn: boolean;
  signal: PasteSignal;
  takePaste(): HeldPaste | null;
  /** The draft; a notice about an earlier draft clears once it changes. */
  text: string;
  /** Busy while a send is in flight; a send may consume the batch. */
  disabled: boolean;
  /** The ids the field names in aria-describedby. */
  noticeId: string;
  reasonId: string;
  onStaged(next: StagedSummary): void;
}) {
  const [state, setState] = useState(EMPTY);
  const [notice, setNotice] = useState<{ text: string; message: string }>();
  const ref = useRef(state);
  const settled = useRef(false);
  const handled = useRef(0);
  // Keyed to the draft it was made for, so the next edit clears it.
  const announce = useCallback(
    (message: string) => setNotice({ text, message }),
    [text],
  );
  const owner = useMemo<StagedOwner>(() => {
    const publish = (next: PastedTextState) =>
      onStaged({
        revision: next.items.length ? next.revision : null,
        busy: next.busy,
        settled: settled.current,
      });
    return {
      chatId,
      ref,
      set(next) {
        ref.current = next;
        setState(next);
        publish(next);
      },
      settle() {
        settled.current = true;
        publish(ref.current);
      },
    };
  }, [chatId, onStaged]);
  // On load, on a chat change and after each send, which may consume it.
  // Until the first read settles, a send could miss a batch staged before
  // a reload, so the composer holds sends until then.
  useEffect(() => {
    if (disabled) return;
    if (!isTauriRuntime()) return owner.settle();
    void reconcile(owner).finally(() => owner.settle());
  }, [owner, disabled]);
  // Nothing staged here may outlive this chat's composer.
  useEffect(
    () => () => onStaged({ revision: null, busy: false, settled: true }),
    [onStaged],
  );
  const attachability = useMemo(
    () => ({ nativeAcp, session }),
    [nativeAcp, session],
  );
  useEffect(() => {
    // Each signal is handled once, also under a doubled effect.
    if (signal.seq === handled.current) return;
    handled.current = signal.seq;
    if (signal.kind === "edit") {
      const refusal = pasteAttachmentRefusal(
        isTauriRuntime() ? attachability : { nativeAcp: false, session: null },
      );
      // Announced after the event, like a native refusal would be.
      void Promise.resolve(typedOverflowMessage(signal.bytes, refusal)).then(
        announce,
      );
      return;
    }
    const held = signal.kind === "paste" ? takePaste() : null;
    if (held) void pasteOver(owner, attachability, held, announce, disabled);
  }, [announce, attachability, disabled, owner, signal, takePaste]);
  const transient = notice?.text === text ? notice.message : "";
  const reason =
    activeTurn && state.items.length
      ? "Attached text goes with the next message you send, not a queued one. Send it once the Turn finishes."
      : "";
  return (
    <>
      <div
        id={noticeId}
        className={transient ? "composer__disabled-reason" : ""}
        aria-live="polite"
      >
        {transient}
      </div>
      {reason ? (
        <p id={reasonId} className="composer__disabled-reason">
          {reason}
        </p>
      ) : null}
      {state.items.length > 0 ? (
        <PastedTextChips
          items={state.items}
          // A removal racing a paste is refused by revision natively.
          disabled={disabled || state.busy}
          onRemove={(id, label) =>
            void removeStaged(owner, id, label, announce)
          }
        />
      ) : null}
    </>
  );
}
