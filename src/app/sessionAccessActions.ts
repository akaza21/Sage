import {
  SESSION_ACCESS_MODES,
  type SessionAccessMode,
} from "../domain/sessionAccessMode";
import { create } from "zustand";
import { chatAccessChanges } from "../ports/chatAccessChanges";

// Loaded on demand, with the access menu or a live chat's access, so the
// chat path carries none of it.
interface ChatAccessStore {
  /** Chats' own modes by session (null: the default), newest last. */
  modes: Readonly<Record<string, SessionAccessMode | null>>;
  /** The last chat access change's failure; it never blocks sending. */
  error: string | null;
}
export const useChatAccessStore = create<ChatAccessStore>(() => ({
  modes: {},
  error: null,
}));
const store = useChatAccessStore;
/** Chats whose mode is held; older ones are read again when shown. */
const MAX_CHAT_MODES = 32;

/** Records a chat's mode, keeping the newest `MAX_CHAT_MODES`. */
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

/** Reads a chat's own mode from the native store. */
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
 * Gives the chat a mode that needs no confirmation. The native store
 * replaces whatever the chat had, Full access included, so stepping down
 * never depends on what this window believes the chat holds.
 */
export async function chooseChatAccess(
  sessionId: string,
  mode: SessionAccessMode,
): Promise<void> {
  try {
    await chatAccessChanges().set(sessionId, mode);
    record(sessionId, mode);
  } catch {
    store.setState({
      error: "This chat's access could not be changed. Try again.",
    });
    await loadChatAccess(sessionId);
  }
}

/** Opens the confirmation of a permissive mode; resolves to its token. */
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

/** Spends the confirmation's token to give the chat a permissive mode. */
export async function grantChatAccess(
  sessionId: string,
  mode: SessionAccessMode,
  confirmToken: string,
): Promise<void> {
  try {
    await chatAccessChanges().grant(sessionId, mode, confirmToken);
    record(sessionId, mode);
  } catch {
    store.setState({
      error: "This access was not granted. Confirm again.",
    });
    // Show what the chat really holds after the refusal.
    await loadChatAccess(sessionId);
  }
}
