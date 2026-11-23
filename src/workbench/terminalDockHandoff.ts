import { useEffect, type RefObject } from "react";
import { useWorkbenchStore, type TerminalHandoff } from "../app/store";
import type { UserTerminalScope } from "../services/userTerminalDesktop";

/**
 * Takes the pending terminal handoff while the dock is shown and idle, and
 * hands it to `create`, which opens a new tab, writes the input and settles
 * it. With no handoff, a shown dock without tabs opens one plain terminal.
 */
export function useTerminalHandoff(
  scope: UserTerminalScope,
  visible: boolean,
  busy: boolean,
  entries: number,
  ownerRef: RefObject<{ lifecycle: number; entries: unknown[] }>,
  create: (handoff?: TerminalHandoff) => Promise<void>,
) {
  const pending = useWorkbenchStore((state) => state.terminalHandoff);
  useEffect(() => {
    const owner = ownerRef.current;
    const lifecycle = owner.lifecycle;
    if (!visible || busy || (entries && !pending)) return;
    void Promise.resolve().then(() => {
      if (owner.lifecycle !== lifecycle) return;
      const handoff = useWorkbenchStore.getState().terminalHandoff;
      if (!handoff) {
        if (!owner.entries.length) void create();
        return;
      }
      useWorkbenchStore.setState({ terminalHandoff: null });
      if (handoff.chatId === scope.chatId) void create(handoff);
      else handoff.finish("Select this chat, then try again.");
    });
  }, [create, visible, busy, entries, pending, scope, ownerRef]);
}
