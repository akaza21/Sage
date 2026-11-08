import {
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
  type RefObject,
} from "react";

import { isTauriRuntime } from "../../ports/nativeInvoke";
import {
  fitsComposer,
  typedOverflowMessage,
  utf8Length,
  type PasteAttachability,
} from "./composerPaste";
import { PastedTextChips } from "./PastedTextChips";
import {
  pasteOver,
  reconcile,
  removeStaged,
  type StagedOwner,
} from "./pastedTextFlow";
import type { PastedTextState, StagedSummary } from "./pastedTextTypes";

const EMPTY: PastedTextState = { revision: null, items: [], busy: false };

/**
 * Lazy: loads with the composer's first focus or over-cap input. Owns the
 * chat's staged pasted text: attaching a paste too long for the composer,
 * refusing one that cannot attach, the announcements, the chips and their
 * preview. The composer keeps only its synchronous size check, which holds
 * back any over-cap edit, a paste racing this part's load included.
 */
export function ComposerPaste({
  chatId,
  fieldRef,
  session,
  nativeAcp,
  activeTurn,
  input,
  text,
  disabled,
  onStaged,
}: {
  chatId: string;
  fieldRef: RefObject<HTMLTextAreaElement | null>;
  session: PasteAttachability["session"];
  nativeAcp: boolean;
  activeTurn: boolean;
  /** An edit held back for the cap; `true` when only focus loaded this. */
  input: string | boolean;
  /** The draft; a notice about an earlier draft clears once it changes. */
  text: string;
  /** Busy while a send is in flight; a send may consume the batch. */
  disabled: boolean;
  onStaged(next: StagedSummary): void;
}) {
  const [state, setState] = useState(EMPTY);
  const [notice, setNotice] = useState<{ text: string; message: string }>();
  const ref = useRef(state);
  const handled = useRef<string | boolean>(true);
  // Keyed to the draft it was made for, so the next edit clears it.
  const announce = useCallback(
    (message: string) => setNotice({ text, message }),
    [text],
  );
  const owner = useMemo<StagedOwner>(
    () => ({
      chatId,
      ref,
      set(next) {
        ref.current = next;
        setState(next);
        onStaged({
          revision: next.items.length ? next.revision : null,
          busy: next.busy,
        });
      },
    }),
    [chatId, onStaged],
  );
  // On load, on a chat change and after each send, which may consume it.
  useEffect(() => {
    if (!disabled && isTauriRuntime()) void reconcile(owner);
  }, [owner, disabled]);
  // Nothing staged here may outlive this chat's composer.
  useEffect(() => () => onStaged({ revision: null, busy: false }), [onStaged]);
  useEffect(() => {
    // Each held-back edit is announced once, also under a doubled effect.
    if (typeof input !== "string" || input === handled.current) return;
    handled.current = input;
    // Announced after the event, like a native refusal would be.
    void Promise.resolve(typedOverflowMessage(utf8Length(input))).then(
      announce,
    );
  }, [announce, input]);
  useEffect(() => {
    const field = fieldRef.current;
    // Runs before the form's own paste handling; image files go on to it.
    const onPaste = (event: ClipboardEvent) => {
      const data = event.clipboardData;
      if (!field || !data || data.files.length) return;
      const value = data.getData("text/plain");
      const before = field.value.slice(0, field.selectionStart);
      const after = field.value.slice(field.selectionEnd);
      if (fitsComposer(before, value, after)) return;
      // Decided synchronously: the browser never inserts an over-cap paste.
      event.preventDefault();
      void pasteOver(
        owner,
        { nativeAcp, session },
        { before, value, after },
        announce,
      );
    };
    field?.addEventListener("paste", onPaste);
    return () => field?.removeEventListener("paste", onPaste);
  }, [announce, fieldRef, nativeAcp, owner, session]);
  const message = [
    notice?.text === text ? notice.message : "",
    activeTurn && state.items.length
      ? "Attached text goes with the next message you send, not a queued one. Send it once the Turn finishes."
      : "",
  ]
    .join(" ")
    .trim();
  return (
    <>
      <div
        className={message ? "composer__disabled-reason" : ""}
        aria-live="polite"
      >
        {message}
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
