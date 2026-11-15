import {
  SESSION_ACCESS_MODES,
  type SessionAccessMode,
} from "../domain/sessionAccessMode";
import { create } from "zustand";
import { chatAccessChanges } from "../ports/chatAccessChanges";

// Loaded on demand, with the access menu or a live chat's access, so the
// chat path carries none of it.
interface ChatAccessStore {
  /** By session; null is the harness default. Newest last. */
  modes: Readonly<Record<string, SessionAccessMode | null>>;
  error: string | null;
}
export const useChatAccessStore = create<ChatAccessStore>(() => ({
  modes: {},
  error: null,
}));
const store = useChatAccessStore;
/** Modes that run actions without asking take a confirmed native token. */
export const PERMISSIVE_ACCESS: readonly SessionAccessMode[] = [
  "auto",
  "fullAccess",
];
const MAX_CHAT_MODES = 32;

export function withChatMode(
  modes: Readonly<Record<string, SessionAccessMode | null>>,
  sessionId: string,
  mode: unknown,
) {
  const admitted = SESSION_ACCESS_MODES.find((known) => known === mode) ?? null;
  const { [sessionId]: _replaced, ...rest } = modes;
  return Object.fromEntries(
    [...Object.entries(rest), [sessionId, admitted] as const].slice(
      -MAX_CHAT_MODES,
    ),
  );
}

export async function loadChatAccess(sessionId: string): Promise<void> {
  try {
    const mode = await chatAccessChanges().get(sessionId);
    store.setState((state) => ({
      modes: withChatMode(state.modes, sessionId, mode),
    }));
  } catch {
    store.setState({ error: "This chat's access could not be loaded." });
  }
}

const record = (sessionId: string, mode: SessionAccessMode) =>
  store.setState((state) => ({
    error: null,
    modes: withChatMode(state.modes, sessionId, mode),
  }));

/**
 * The native store replaces whatever the chat had, so stepping down never
 * depends on what this window believes the chat holds.
 */
export async function chooseChatAccess(
  sessionId: string,
  mode: SessionAccessMode,
): Promise<boolean> {
  try {
    const change = await chatAccessChanges().set(sessionId, mode);
    record(sessionId, mode);
    if (change?.turnCancelled) {
      store.setState({
        error:
          "The running turn was stopped because this chat's access was lowered.",
      });
    }
    return true;
  } catch {
    store.setState({
      error: "This chat's access could not be changed. Try again.",
    });
    await loadChatAccess(sessionId);
    return false;
  }
}

export async function prepareChatAccess(
  sessionId: string,
  mode: SessionAccessMode,
): Promise<string | null> {
  try {
    const { confirmToken } = await chatAccessChanges().prepare(sessionId, mode);
    return confirmToken;
  } catch {
    store.setState({
      error: "The confirmation could not be opened. Try again.",
    });
    return null;
  }
}

export async function grantChatAccess(
  sessionId: string,
  mode: SessionAccessMode,
  confirmToken: string,
): Promise<boolean> {
  try {
    await chatAccessChanges().grant(sessionId, mode, confirmToken);
    record(sessionId, mode);
    return true;
  } catch {
    store.setState({
      error: "This access was not granted. Confirm again.",
    });
    await loadChatAccess(sessionId);
    return false;
  }
}

export async function applyChatAccessDraft(
  sessionId: string,
  mode: SessionAccessMode,
): Promise<boolean> {
  if (!PERMISSIVE_ACCESS.includes(mode))
    return chooseChatAccess(sessionId, mode);
  const token = await prepareChatAccess(sessionId, mode);
  return token !== null && grantChatAccess(sessionId, mode, token);
}
