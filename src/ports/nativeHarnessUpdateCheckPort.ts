import { invokeNative, isTauriRuntime } from "./nativeInvoke";

// The browser lane runs no npm check; it keeps the switch for the page's
// lifetime so the control behaves the same.
let browserEnabled = true;

/** Settings › Harnesses switch; see `settings_commands/harness_update_check.rs`. */
export function harnessUpdateCheck(): Promise<boolean> {
  return isTauriRuntime()
    ? invokeNative("harness_update_check")
    : Promise.resolve(browserEnabled);
}

export function harnessSetUpdateCheck(enabled: boolean): Promise<boolean> {
  if (isTauriRuntime())
    return invokeNative("harness_set_update_check", { enabled });
  browserEnabled = enabled;
  return Promise.resolve(enabled);
}
