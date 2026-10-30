import type { BrowserElementContext } from "../domain/attachedContext";
import type { BrowserAnnotateCrop } from "../ports/nativeBrowserAnnotatePort";

// Each bounded draft owns its previews. Never retain a closed chat or evict
// another draft's active picture to make room for the current chat.
const previews = new WeakMap<BrowserElementContext, BrowserAnnotateCrop>();
let slots: WeakRef<BrowserElementContext>[] = [];

export function rememberBrowserCrop(
  context: BrowserElementContext,
  crop: BrowserAnnotateCrop,
) {
  slots = slots.filter((slot) => {
    const entry = slot.deref();
    return entry && previews.has(entry);
  });
  if (!previews.has(context)) {
    if (slots.length === 8) return false;
    slots.push(new WeakRef(context));
  }
  previews.set(context, crop);
  return true;
}

export const browserCropPreview = (context: BrowserElementContext) =>
  previews.get(context);
export const forgetBrowserCrop = (context: BrowserElementContext) =>
  previews.delete(context);
