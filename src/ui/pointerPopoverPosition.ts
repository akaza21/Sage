import type { CSSProperties } from "react";

import { GUTTER } from "./menuPopoverPosition";

/** A viewport point, such as where a right-click landed. */
export interface PopoverPoint {
  x: number;
  y: number;
}

/**
 * Opens a context menu at the pointer, as a native one does: its top-left
 * corner at the point, flipped to the left of or above the point where the
 * viewport has no room after it, and clamped inside the viewport otherwise.
 */
export function pointerPopoverPosition(
  point: PopoverPoint,
  popover: HTMLElement,
): CSSProperties {
  const surface = popover.getBoundingClientRect();
  const along = (start: number, size: number, limit: number) => {
    const far = limit - GUTTER;
    const extent = Math.min(size, far - GUTTER);
    const at = start + extent <= far ? start : start - extent;
    return Math.max(GUTTER, Math.min(at, far - extent));
  };
  return {
    left: along(point.x, surface.width, window.innerWidth),
    top: along(point.y, surface.height, window.innerHeight),
    maxWidth: Math.max(0, window.innerWidth - 2 * GUTTER),
    visibility: "visible",
  };
}
