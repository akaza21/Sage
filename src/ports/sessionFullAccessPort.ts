import { invokeNative, isTauriRuntime } from "./nativeInvoke";

/**
 * Native Full access changes (see `session_access.rs`). Reading the granted
 * sessions lives in `fullAccessSessions.ts`, so only the access menu loads
 * this.
 */
export interface SessionFullAccessPort {
  /** Opens a confirmation and returns the single-use token it expects. */
  prepare(sessionId: string): Promise<{ confirmToken: string }>;
  grant(sessionId: string, confirmToken: string): Promise<void>;
  revoke(sessionId: string): Promise<void>;
}

const native: SessionFullAccessPort = {
  prepare: (sessionId) =>
    invokeNative("prepare_session_full_access", { request: { sessionId } }),
  grant: (sessionId, confirmToken) =>
    invokeNative("grant_session_full_access", {
      request: { sessionId, confirmToken },
    }),
  revoke: (sessionId) =>
    invokeNative("revoke_session_full_access", { request: { sessionId } }),
};

const unavailable: SessionFullAccessPort = {
  prepare: () => Promise.reject(new Error("Full access needs Sage desktop.")),
  grant: () => Promise.reject(new Error("Full access needs Sage desktop.")),
  revoke: () => Promise.resolve(),
};

export function sessionFullAccessPort(): SessionFullAccessPort {
  return isTauriRuntime() ? native : unavailable;
}
