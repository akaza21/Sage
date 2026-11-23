import { useEffect, type RefObject } from "react";
import {
  useTerminalDockStore,
  type TerminalHandoff,
} from "../app/terminalDockStore";
import { utf8ByteLength } from "../domain/text";
import {
  TERMINAL_INPUT_BYTES,
  type UserTerminalScope,
} from "../services/userTerminalDesktop";

// Typed input only: one line within the native input bound, with no control
// character, so nothing reaches the shell as Enter until the user presses it.
const CONTROL = /\p{Cc}/u;

/** The prefill error for `text`, or null when it may be written as typed input. */
export function prefillError(text: string): string | null {
  return text &&
    utf8ByteLength(text) <= TERMINAL_INPUT_BYTES &&
    !CONTROL.test(text)
    ? null
    : `Terminal input must be one line of at most ${TERMINAL_INPUT_BYTES} UTF-8 bytes.`;
}

/**
 * Takes the pending handoff while the dock is shown and hands it to `create`,
 * which opens a new tab, writes the input and settles it. With no handoff, a
 * shown dock without tabs opens one plain terminal, as before.
 */
export function useTerminalHandoff(
  scope: UserTerminalScope,
  visible: boolean,
  entries: number,
  ownerRef: RefObject<{ lifecycle: number; entries: unknown[] }>,
  create: (handoff?: TerminalHandoff) => Promise<void>,
) {
  const pending = useTerminalDockStore((state) => state.handoff);
  useEffect(() => {
    const owner = ownerRef.current;
    const lifecycle = owner.lifecycle;
    if (!visible || (entries && !pending)) return;
    void Promise.resolve().then(() => {
      if (owner.lifecycle !== lifecycle) return;
      const handoff = useTerminalDockStore.getState().handoff;
      if (!handoff) {
        if (!owner.entries.length) void create();
        return;
      }
      useTerminalDockStore.setState({ handoff: null });
      const error =
        handoff.chatId === scope.chatId
          ? prefillError(handoff.text)
          : "Select the originating chat before opening its terminal.";
      if (error) handoff.finish(error);
      else void create(handoff);
    });
  }, [create, visible, entries, pending, scope, ownerRef]);
}
