import { invokeNative, isTauriRuntime } from "./nativeInvoke";

/** Most grants the native store keeps (`MAX_FULL_ACCESS_SESSIONS`). */
export const MAX_FULL_ACCESS_SESSIONS = 64;

/** Sessions holding a Full access grant, most recent first. */
export function listFullAccessSessions(): Promise<string[]> {
  // Without the desktop runtime no harness can hold Full access.
  return isTauriRuntime()
    ? invokeNative<string[]>("get_session_full_access")
    : Promise.resolve([]);
}
