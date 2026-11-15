import { invokeNative } from "./nativeInvoke";

// The comment box's commands, kept out of `nativeBrowserPort.ts` so they load
// with the comment box rather than with the core bundle.

export interface BrowserAnnotateFrame {
  jpegBase64: string;
  width: number;
  height: number;
}

export function browserAnnotateFrame(
  tabId: string,
): Promise<BrowserAnnotateFrame> {
  return invokeNative("browser_control", { tabId, action: "annotateFrame" });
}

export function browserAnnotateConfirm(
  tabId: string,
  ordinal: number,
  comment: string,
): Promise<unknown> {
  return invokeNative("browser_control", {
    tabId,
    action: "annotateConfirm",
    ordinal,
    comment,
  });
}
