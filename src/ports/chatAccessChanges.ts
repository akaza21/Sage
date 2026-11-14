import type { SessionAccessMode } from "../domain/sessionAccessMode";
import { invokeNative, isTauriRuntime } from "./nativeInvoke";

/**
 * One chat's access (see `session_access.rs`). Loaded on demand, never on
 * the chat path.
 */
export interface ChatAccessChanges {
  get(sessionId: string): Promise<unknown>;
  /**
   * Sets a mode that needs no confirmation; it replaces any other. Stepping
   * down from Full access or Auto stops a running turn, and says so.
   */
  set(
    sessionId: string,
    mode: SessionAccessMode,
  ): Promise<{ turnCancelled?: boolean } | null | undefined>;
  prepare(
    sessionId: string,
    mode: SessionAccessMode,
  ): Promise<{ confirmToken: string }>;
  grant(
    sessionId: string,
    mode: SessionAccessMode,
    confirmToken: string,
  ): Promise<void>;
}

const native: ChatAccessChanges = {
  get: (sessionId) =>
    invokeNative("get_chat_access", { request: { sessionId } }),
  set: (sessionId, mode) =>
    invokeNative("set_chat_access", { request: { sessionId, mode } }),
  prepare: (sessionId, mode) =>
    invokeNative("prepare_chat_access", { request: { sessionId, mode } }),
  grant: (sessionId, mode, confirmToken) =>
    invokeNative("grant_chat_access", {
      request: { sessionId, mode, confirmToken },
    }),
};

const desktopOnly = () =>
  Promise.reject(new Error("Chat access needs Sage desktop."));
const unavailable: ChatAccessChanges = {
  get: () => Promise.resolve(null),
  set: desktopOnly,
  prepare: desktopOnly,
  grant: desktopOnly,
};

export function chatAccessChanges(): ChatAccessChanges {
  return isTauriRuntime() ? native : unavailable;
}
