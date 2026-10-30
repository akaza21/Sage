import { invokeNative } from "./nativeInvoke";

// The comment box's commands, kept out of `nativeBrowserPort.ts` so they load
// with the comment box rather than with the core bundle.

/** What the page area showed when the element was picked; it covers the whole area. */
export interface BrowserAnnotateFrame {
  jpegBase64: string;
  pngBase64?: string;
  width: number;
  height: number;
}

export interface BrowserAnnotateCrop {
  pngBase64: string;
  width: number;
  height: number;
}

export async function browserAnnotateCrop(
  tabId: string,
): Promise<BrowserAnnotateCrop> {
  const crop = await invokeNative<BrowserAnnotateCrop>("browser_control", {
    tabId,
    action: "annotateCrop",
  });
  if (
    !crop ||
    !Number.isInteger(crop.width) ||
    !Number.isInteger(crop.height) ||
    crop.width < 1 ||
    crop.height < 1 ||
    crop.width > 1024 ||
    crop.height > 1024 ||
    typeof crop.pngBase64 !== "string" ||
    crop.pngBase64.length > 5_592_408 ||
    !/^iVBORw0KGgo[A-Za-z0-9+/]*={0,2}$/.test(crop.pngBase64)
  )
    throw new Error("Invalid masked area preview");
  return crop;
}

export function browserAnnotateFrame(
  tabId: string,
): Promise<BrowserAnnotateFrame> {
  return invokeNative("browser_control", { tabId, action: "annotateFrame" });
}

/** Rust numbers, comments and checks the waiting element, then answers with the item. */
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
