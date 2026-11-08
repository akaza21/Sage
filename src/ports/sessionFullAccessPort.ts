import { invokeNative, isTauriRuntime } from "./nativeInvoke";

/** Native store of per-session Full access grants (see `session_access.rs`). */
export interface SessionFullAccessPort {
  /** Granted session ids, most recent first. */
  list(): Promise<string[]>;
  /** Opens a confirmation and returns the single-use token it expects. */
  prepare(sessionId: string): Promise<{ confirmToken: string }>;
  grant(sessionId: string, confirmToken: string): Promise<void>;
  revoke(sessionId: string): Promise<void>;
}

/** Most grants the native store keeps (`MAX_FULL_ACCESS_SESSIONS`). */
export const MAX_FULL_ACCESS_SESSIONS = 64;

const native: SessionFullAccessPort = {
  list: () => invokeNative<string[]>("get_session_full_access"),
  prepare: (sessionId) =>
    invokeNative("prepare_session_full_access", { request: { sessionId } }),
  grant: (sessionId, confirmToken) =>
    invokeNative("grant_session_full_access", {
      request: { sessionId, confirmToken },
    }),
  revoke: (sessionId) =>
    invokeNative("revoke_session_full_access", { request: { sessionId } }),
};

// Without the desktop runtime no harness can hold Full access.
const unavailable: SessionFullAccessPort = {
  list: () => Promise.resolve([]),
  prepare: () => Promise.reject(new Error("Full access needs Sage desktop.")),
  grant: () => Promise.reject(new Error("Full access needs Sage desktop.")),
  revoke: () => Promise.resolve(),
};

export function sessionFullAccessPort(): SessionFullAccessPort {
  return isTauriRuntime() ? native : unavailable;
}
