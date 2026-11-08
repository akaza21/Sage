import {
  SESSION_ACCESS_MODES,
  type SessionAccessMode,
} from "../domain/sessionAccessMode";
import { chatAccessChanges } from "../ports/chatAccessChanges";
import { useSessionAccessModeStore } from "./sessionAccessModeStore";

// Loaded on demand, with the access menu or when a chat is first shown, so
// the chat path carries none of it.
const store = useSessionAccessModeStore;
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
      chatModes: withChatMode(state.chatModes, sessionId, mode),
    }));
  } catch {
    store.setState({ chatError: "This chat's access could not be loaded." });
  }
}

const record = (sessionId: string, mode: SessionAccessMode) =>
  store.setState((state) => ({
    chatError: null,
    chatModes: withChatMode(state.chatModes, sessionId, mode),
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
      chatError: "This chat's access could not be changed. Try again.",
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
      chatError: "The confirmation could not be opened. Try again.",
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
      chatError: "This access was not granted. Confirm again.",
    });
    // Show what the chat really holds after the refusal.
    await loadChatAccess(sessionId);
  }
}
