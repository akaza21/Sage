import { invokeNative } from "../../ports/nativeInvoke";

import {
  parseQuickOpenResponse,
  type QuickOpenInvoke,
  type QuickOpenRequest,
  type QuickOpenResponse,
} from "./quickOpenDomain";

function invokeDesktop(command: string, args: Record<string, unknown>) {
  if (typeof window === "undefined" || !("__TAURI_INTERNALS__" in window)) {
    return Promise.reject(
      new Error("Quick Open is available in the Sage desktop runtime."),
    );
  }
  return invokeNative(command, args);
}

let invoke: QuickOpenInvoke = invokeDesktop;

/** Replaces the native command, for the browser runtime's test fixtures. */
export function setQuickOpenInvoke(next: QuickOpenInvoke | null) {
  invoke = next ?? invokeDesktop;
}

export async function queryQuickOpen(
  request: QuickOpenRequest,
): Promise<QuickOpenResponse> {
  return parseQuickOpenResponse(await invoke("quick_open_query", { request }));
}
