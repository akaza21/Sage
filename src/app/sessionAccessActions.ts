import {
  FULL_ACCESS,
  type SessionAccessMode,
} from "../domain/sessionAccessMode";
import { MAX_FULL_ACCESS_SESSIONS } from "../ports/fullAccessSessions";
import { sessionFullAccessPort } from "../ports/sessionFullAccessPort";
import { useSessionAccessModeStore } from "./sessionAccessModeStore";

// Loaded with the access menu: only a choice made there changes access.
const store = useSessionAccessModeStore;

/** Opens a Full access confirmation; resolves to its single-use token. */
export async function prepareFullAccess(
  sessionId: string,
): Promise<string | null> {
  try {
    const { confirmToken } = await sessionFullAccessPort().prepare(sessionId);
    return confirmToken;
  } catch {
    store.setState({
      fullAccessError: "Full access could not be prepared. Try again.",
    });
    return null;
  }
}

/** Spends the confirmation's token to grant this chat Full access. */
export async function grantFullAccess(
  sessionId: string,
  confirmToken: string,
): Promise<void> {
  try {
    await sessionFullAccessPort().grant(sessionId, confirmToken);
    store.setState((state) => ({
      fullAccessError: null,
      fullAccessSessions: [
        sessionId,
        ...state.fullAccessSessions.filter((id) => id !== sessionId),
      ].slice(0, MAX_FULL_ACCESS_SESSIONS),
    }));
  } catch {
    store.setState({
      fullAccessError: "Full access was not granted. Confirm again.",
    });
  }
}

/** Ends this chat's Full access; false when it is still granted. */
async function revokeFullAccess(sessionId: string): Promise<boolean> {
  try {
    await sessionFullAccessPort().revoke(sessionId);
    store.setState((state) => ({
      fullAccessError: null,
      fullAccessSessions: state.fullAccessSessions.filter(
        (id) => id !== sessionId,
      ),
    }));
    return true;
  } catch {
    store.setState({
      fullAccessError: "Full access could not be turned off. Try again.",
    });
    return false;
  }
}

/**
 * Chooses a mode other than Full access as the binding's default. A chat
 * holding Full access gives it up first, and keeps it if that fails.
 */
export async function chooseAccess(
  bindingId: string,
  sessionId: string | null,
  mode: SessionAccessMode,
): Promise<void> {
  if (mode === FULL_ACCESS) return;
  const granted =
    sessionId !== null &&
    store.getState().fullAccessSessions.includes(sessionId);
  if (granted && !(await revokeFullAccess(sessionId))) return;
  store.getState().setMode(bindingId, mode);
}
