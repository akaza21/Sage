import { useCallback, useEffect, useMemo, useRef, useState } from "react";

import { isTauriRuntime } from "../../ports/nativeInvoke";
import {
  heldPasteRefusal,
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
  input: { seq, held },
  text,
  disabled,
  noticeId,
  onStaged,
}: {
  chatId: string | undefined;
  session: PasteAttachability["session"];
  nativeAcp: boolean;
  activeTurn: boolean;
  /**
   * The composer's held-back input, in order, drained here; seq is bumped
   * for each input held, and is 0 when the part is only armed.
   */
  input: { seq: number; held: { current: HeldPaste[] } };
  /** The draft; a notice about an earlier draft clears once it changes. */
  text: string;
  /** Busy while a send is in flight; a send may consume the batch. */
  disabled: boolean;
  /** The field names it in aria-describedby; it holds every notice. */
  noticeId: string;
  onStaged(next: StagedSummary): void;
}) {
  const [state, setState] = useState(EMPTY);
  const [notice, setNotice] = useState<{ text: string; message: string }>();
  const ref = useRef(state);
  const settled = useRef(false);
  const handled = useRef(0);
  // Held pastes are staged one after another, never two at once.
  const chain = useRef(Promise.resolve());
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
    // Each bump is handled once, also under a doubled effect.
    if (seq === handled.current) return;
    handled.current = seq;
    // Every held input is drained, in order, whatever came last.
    const inputs = held.current.splice(0);
    // Announced after the event, like a native refusal would be.
    chain.current = chain.current.then(async () => {
      for (const held of inputs) {
        if ("edit" in held)
          announce(
            typedOverflowMessage(
              held.edit,
              pasteAttachmentRefusal(
                isTauriRuntime()
                  ? attachability
                  : { nativeAcp: false, session: null },
              ),
            ),
          );
        else if ("bytes" in held) announce(heldPasteRefusal(held));
        else await pasteOver(owner, attachability, held, announce, disabled);
      }
    });
  }, [announce, attachability, disabled, owner, seq, held]);
  const transient = notice?.text === text ? notice.message : "";
  const reason =
    activeTurn && state.items.length
      ? "Attached text goes with the next message you send, not a queued one. Send it once the Turn finishes."
      : "";
  return (
    <>
      <div id={noticeId}>
        <div
          className={transient ? "composer__disabled-reason" : ""}
          aria-live="polite"
        >
          {transient}
        </div>
        {reason ? <p className="composer__disabled-reason">{reason}</p> : null}
      </div>
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
