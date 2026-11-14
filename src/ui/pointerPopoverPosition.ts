import type { CSSProperties } from "react";

import { GUTTER } from "./menuPopoverPosition";

export interface PopoverPoint {
  x: number;
  y: number;
}

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
