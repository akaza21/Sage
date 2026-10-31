import type { AttachedContext } from "../domain/attachedContext";

type ReleasePictures = (
  contexts: readonly AttachedContext[],
  kept: readonly AttachedContext[],
) => void;

// Preview storage loads with browser UI. Draft cleanup stays synchronous even
// when no browser has ever opened; there cannot be pictures before registration.
export const browserCropLifecycle: { release?: ReleasePictures } = {};
