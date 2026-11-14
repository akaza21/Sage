import { useState } from "react";

export interface Point {
  x: number;
  y: number;
}

export const SUBMENU_GRACE_MS = 300;
const BLEED = 5;

export function graceArea(exit: Point, submenu: DOMRect): Point[] | null {
  if (exit.x > submenu.left && exit.x < submenu.right) return null;
  const right = submenu.left >= exit.x;
  const near = right ? submenu.left : submenu.right;
  const far = right ? submenu.right : submenu.left;
  return [
    { x: exit.x + (right ? -BLEED : BLEED), y: exit.y },
    { x: near, y: submenu.top },
    { x: far, y: submenu.top },
    { x: far, y: submenu.bottom },
    { x: near, y: submenu.bottom },
  ];
}

export function pointInPolygon(point: Point, polygon: readonly Point[]) {
  let inside = false;
  for (let i = 0, j = polygon.length - 1; i < polygon.length; j = i++) {
    const a = polygon[i];
    const b = polygon[j];
    if (
      a.y > point.y !== b.y > point.y &&
      point.x < ((b.x - a.x) * (point.y - a.y)) / (b.y - a.y) + a.x
    )
      inside = !inside;
  }
  return inside;
}

type PointerPosition = { clientX: number; clientY: number };

export interface SubmenuGrace {
  leave(event: PointerPosition, submenu: Element | null): void;
  holds(event: PointerPosition): boolean;
  reset(): void;
}

export function useSubmenuGrace(): SubmenuGrace {
  const [grace] = useState(() => {
    let area: Point[] | null = null;
    let until = 0;
    return {
      leave(event: PointerPosition, submenu: Element | null) {
        area = submenu
          ? graceArea(
              { x: event.clientX, y: event.clientY },
              submenu.getBoundingClientRect(),
            )
          : null;
        until = performance.now() + SUBMENU_GRACE_MS;
      },
      holds(event: PointerPosition) {
        if (
          area &&
          performance.now() < until &&
          pointInPolygon({ x: event.clientX, y: event.clientY }, area)
        )
          return true;
        area = null;
        return false;
      },
      reset() {
        area = null;
      },
    };
  });
  return grace;
}
