import { useCallback, useEffect, useMemo, useRef, useState } from "react";

import { isTauriRuntime } from "../../ports/nativeInvoke";
import {
  heldInputMessage,
  pasteAttachmentRefusal,
  type PasteAttachability,
} from "./composerPasteText";
import { ComposerPasteNotices } from "./ComposerPasteNotices";
import { PastedTextChips } from "./PastedTextChips";
import {
  pasteOver,
  pickFiles,
  reconcile,
  removeStaged,
  type StagedOwner,
} from "./pastedTextFlow";
import type {
  HeldInput,
  PastedTextState,
  StagedSummary,
} from "./pastedTextTypes";

const EMPTY: PastedTextState = { revision: null, items: [], busy: false };

/**
 * Lazy: loads with a native chat's composer or with held input. The one
 * owner of the chat's staged batch: attaching a paste too long for the
 * composer, refusing one that cannot attach, files picked with the attach
 * button, the announcements, the chips and their preview. The composer
 * keeps only its synchronous size check and hands over what it held back,
 * a paste or pick made before this part loaded included. Mounted once per
 * chat, so another chat starts empty and reads its own batch back.
 */
export function ComposerPaste({
  chatId,
  session,
  nativeAcp,
  activeTurn,
  input: { seq, take },
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
   * The composer's held-back input: seq is bumped for each input held, and
   * take hands over all of it, oldest first.
   */
  input: { seq: number; take(): HeldInput[] };
  /** The draft; a notice about an earlier draft clears once it changes. */
  text: string;
  /** Busy while a send is in flight; a send may consume the batch. */
  disabled: boolean;
  /** The id the field names in aria-describedby once the part reports. */
  noticeId: string;
  onStaged(next: StagedSummary): void;
}) {
  const [state, setState] = useState(EMPTY);
  const [notice, setNotice] = useState<{ text: string; message: string }>();
  const ref = useRef(state);
  const settled = useRef(false);
  // A result for a chat left meanwhile is dropped; that chat reads its
  // batch back when shown again.
  const live = useRef(true);
  const handled = useRef(0);
  // Held input is answered in order, a paste only once the one before it
  // was staged or refused.
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
        if (!live.current) return;
        ref.current = next;
        setState(next);
        publish(next);
      },
      settle() {
        if (!live.current) return;
        settled.current = true;
        publish(ref.current);
      },
    };
  }, [chatId, onStaged]);
  // Nothing staged here may outlive this chat's composer, and a send waits
  // for this chat's batch, not the one the composer showed before.
  useEffect(() => {
    live.current = true;
    onStaged({ revision: null, busy: false, settled: settled.current });
    return () => {
      live.current = false;
      onStaged({ revision: null, busy: false, settled: true });
    };
  }, [onStaged]);
  // On load and after each send, which may consume it. Until the first read
  // settles, a send could miss a batch staged before a reload, so the
  // composer holds sends until then. Queued behind any staging, so a pick
  // or paste names the revision it replaces.
  useEffect(() => {
    if (disabled) return;
    if (!isTauriRuntime()) return owner.settle();
    const read = chain.current.then(() => reconcile(owner));
    chain.current = read;
    void read.finally(() => owner.settle());
  }, [owner, disabled]);
  const attachability = useMemo(
    () => ({ nativeAcp, session }),
    [nativeAcp, session],
  );
  useEffect(() => {
    // Each bump is handled once, also under a doubled effect.
    if (seq === handled.current) return;
    handled.current = seq;
    // Everything held is taken, whatever came last.
    const inputs = take();
    const refusal = pasteAttachmentRefusal(
      isTauriRuntime() ? attachability : { nativeAcp: false, session: null },
    );
    // Announced after the event, like a native refusal would be.
    chain.current = chain.current.then(async () => {
      for (const held of inputs) {
        if (held.kind === "paste")
          await pasteOver(owner, attachability, held.paste, announce, disabled);
        else if (held.kind === "pick")
          await pickFiles(owner, announce, disabled);
        else announce(heldInputMessage(held, refusal));
      }
    });
  }, [announce, attachability, disabled, owner, seq, take]);
  const transient = notice?.text === text ? notice.message : "";
  const reason =
    activeTurn && state.items.length
      ? "Attached text goes with the next message you send, not a queued one. Send it once the Turn finishes."
      : "";
  return (
    <>
      <ComposerPasteNotices id={noticeId} notice={transient} reason={reason} />
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
