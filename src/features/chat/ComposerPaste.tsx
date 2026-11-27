import {
  useCallback,
  useEffect,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
} from "react";

import { useWorkbenchStore } from "../../app/store";
import { Stacked } from "../../ui/NoticeStack";
import { isTauriRuntime } from "../../ports/nativeInvoke";
import {
  heldInputMessage,
  pasteAttachmentRefusal,
  type PasteAttachability,
} from "./composerPasteText";
import {
  ComposerPasteNotices,
  type PasteNotices,
} from "./ComposerPasteNotices";
// Also registers the draft's file handoff on the draft port.
import { noteDraftFiles } from "./draftFileHandoff";
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

export function ComposerPaste({
  chatId,
  draft,
  session,
  nativeAcp,
  activeTurn,
  input: { seq, take },
  text,
  disabled,
  noticeId,
  onNotices,
  onStaged,
}: {
  chatId: string | undefined;
  /** The new-chat draft, which stages by its own token until a chat exists. */
  draft?: boolean;
  session: PasteAttachability["session"];
  nativeAcp: boolean;
  activeTurn: boolean;
  input: { seq: number; take(): HeldInput[] };
  text: string;
  disabled: boolean;
  noticeId: string;
  /** The composer shows these in its notice stack. */
  onNotices(next: PasteNotices | undefined): void;
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
  const announce = useCallback(
    (message: string) => setNotice({ text, message }),
    [text],
  );
  // The draft's slot is named by its token: a new or retargeted draft reads
  // its own, empty slot.
  const draftToken = useWorkbenchStore((state) =>
    draft ? state.newChatDraft?.fileToken : undefined,
  );
  const owner = useMemo<StagedOwner>(() => {
    const publish = (next: PastedTextState) => {
      const revision = next.items.length ? next.revision : null;
      onStaged({ revision, busy: next.busy, settled: settled.current });
      if (draft) noteDraftFiles(draftToken, revision);
    };
    return {
      chatId: draft ? draftToken : chatId,
      draft,
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
  }, [chatId, draft, draftToken, onStaged]);
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
    const inputs = take();
    const refusal = pasteAttachmentRefusal(
      isTauriRuntime() ? attachability : { nativeAcp: false, session: null },
    );
    chain.current = chain.current.then(async () => {
      for (const held of inputs) {
        if (held.kind === "paste")
          await pasteOver(owner, attachability, held.paste, announce, disabled);
        else if (held.kind === "pick")
          await pickFiles(owner, announce, disabled, held.pick, held.images);
        else announce(heldInputMessage(held, refusal));
      }
    });
  }, [announce, attachability, disabled, owner, seq, take]);
  const transient = notice?.text === text ? notice.message : "";
  const reason =
    activeTurn && state.items.length
      ? "Attached text goes with the next message you send, not a queued one. Send it once the turn finishes."
      : "";
  // Before paint, so the stack never shows a notice a frame late.
  useLayoutEffect(
    () =>
      onNotices(
        <Stacked key={noticeId} quiet={!transient && !reason}>
          <ComposerPasteNotices
            id={noticeId}
            notice={transient}
            reason={reason}
          />
        </Stacked>,
      ),
    [onNotices, noticeId, transient, reason],
  );
  useLayoutEffect(() => () => onNotices(undefined), [onNotices]);
  return state.items.length > 0 ? (
    <PastedTextChips
      items={state.items}
      // A removal racing a paste is refused by revision natively.
      disabled={disabled || state.busy}
      onRemove={(id, label) => void removeStaged(owner, id, label, announce)}
    />
  ) : null;
}
