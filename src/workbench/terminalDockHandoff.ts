import { useTerminalDockStore } from "../app/terminalDockStore";
import { useWorkbenchStore } from "../app/store";
import { utf8ByteLength } from "../domain/text";
import { useEffect, type RefObject } from "react";
import type { TerminalOpenRequest } from "../app/terminalDockStore";
import type { UserTerminalScope } from "../services/userTerminalDesktop";
import type * as TerminalDesktop from "../services/userTerminalDesktop";

type TerminalPort = Pick<
  typeof TerminalDesktop,
  "createUserTerminal" | "writeUserTerminal" | "closeUserTerminal"
>;

/** The dock supplies its existing native adapter; this API never loads PTY code. */
export async function createDockSession(
  scope: UserTerminalScope,
  live: () => boolean,
  request: TerminalOpenRequest | undefined,
  port: TerminalPort,
) {
  const session = await port.createUserTerminal(scope, 80, 24);
  const current = () =>
    live() &&
    (!request || useTerminalDockStore.getState().pendingOpen === request);
  try {
    if (!current()) throw new Error("Terminal opening was cancelled.");
    if (request) await port.writeUserTerminal(scope, session, request.text);
    if (!current()) throw new Error("Terminal opening was cancelled.");
    return session;
  } catch (error) {
    await port.closeUserTerminal(scope, session).catch(() => undefined);
    throw error;
  }
}

export function useTerminalOpen(
  scope: UserTerminalScope,
  visible: boolean,
  entries: number,
  ownerRef: RefObject<{ lifecycle: number; entries: unknown[] }>,
  create: (request?: TerminalOpenRequest) => Promise<void>,
) {
  const pending = useTerminalDockStore((state) => state.pendingOpen);
  useEffect(() => {
    const owner = ownerRef.current;
    const lifecycle = owner.lifecycle;
    if (!visible || (entries && !pending)) return;
    void Promise.resolve().then(() => {
      if (owner.lifecycle !== lifecycle) return;
      if (pending) {
        if (
          pending.scope.projectId !== scope.projectId ||
          pending.scope.projectGeneration !== scope.projectGeneration ||
          pending.scope.chatId !== scope.chatId
        )
          pending.finish(
            new Error(
              "Select the originating chat before opening its terminal.",
            ),
          );
        else void create(pending);
      } else if (!owner.entries.length) void create();
    });
  }, [create, visible, entries, pending, scope, ownerRef]);
}

// Prefills share the native terminal's 4096-byte input bound, without loading
// its I/O adapter before the dock. Reject controls so input cannot press Enter.
const MAX_PREFILL_BYTES = 4096;

/** Native creation resolves the originating chat's project scope to its root cwd. */
export async function openTerminalWith({
  chatId,
  text,
}: {
  chatId: string;
  text: string;
}): Promise<void> {
  const state = useWorkbenchStore.getState();
  const dock = useTerminalDockStore.getState();
  if (!dock.open || !dock.ready)
    throw new Error("Terminal dock is unavailable or still loading.");
  if (
    !state.project ||
    state.projectTransitioning ||
    !state.tabs.some((tab) => tab.kind === "chat" && tab.id === chatId)
  )
    throw new Error("Open this chat's project before opening a terminal.");
  if (!text || utf8ByteLength(text) > MAX_PREFILL_BYTES)
    throw new Error(
      "Terminal input must be one line of at most 4096 UTF-8 bytes.",
    );
  for (const character of text) {
    const code = character.charCodeAt(0);
    if (code < 32 || (code >= 127 && code <= 159))
      throw new Error(
        "Terminal input must be one line of at most 4096 UTF-8 bytes.",
      );
  }
  if (dock.pendingOpen) throw new Error("A terminal is already opening.");
  await new Promise<void>((resolve, reject) => {
    const pendingOpen = {
      scope: {
        projectId: state.project!.id,
        projectGeneration: String(state.nativeTransitionEpoch),
        chatId,
      },
      text,
      finish: (error?: Error) => {
        if (useTerminalDockStore.getState().pendingOpen !== pendingOpen) return;
        clearTimeout(timeout);
        useTerminalDockStore.setState({ pendingOpen: null });
        if (error) reject(error);
        else resolve();
      },
    };
    const timeout = setTimeout(
      () =>
        pendingOpen.finish(
          new Error("Terminal did not become available. Try opening it again."),
        ),
      10_000,
    );
    useTerminalDockStore.setState({ pendingOpen });
    dock.open!();
  });
}
