import type { SessionAccessMode } from "../domain/sessionAccessMode";
import { invokeNative, isTauriRuntime } from "./nativeInvoke";

/**
 * One chat's access (see `session_access.rs`). Loaded on demand, never on
 * the chat path.
 */
export interface ChatAccessChanges {
  /** The chat's own mode, or null when it uses its harness default. */
  get(sessionId: string): Promise<unknown>;
  /** Sets a mode that needs no confirmation; it replaces any other. */
  set(sessionId: string, mode: SessionAccessMode): Promise<void>;
  /** Opens a confirmation and returns the single-use token it expects. */
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
