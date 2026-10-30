import type { BrowserElementContext } from "../../domain/attachedContext";
import type { BrowserAnnotateCrop } from "../../ports/nativeBrowserAnnotatePort";

const previews = new Map<BrowserElementContext, BrowserAnnotateCrop>();

export function rememberBrowserCrop(
  context: BrowserElementContext,
  crop: BrowserAnnotateCrop,
) {
  if (previews.size === 8) previews.delete(previews.keys().next().value!);
  previews.set(context, crop);
}

export const browserCropPreview = (context: BrowserElementContext) =>
  previews.get(context);
export const forgetBrowserCrop = (context: BrowserElementContext) =>
  previews.delete(context);
