import { useState } from "react";

export interface Point {
  x: number;
  y: number;
}

/** How long a move towards an open submenu keeps it open over siblings. */
export const SUBMENU_GRACE_MS = 300;
const BLEED = 5;

/**
 * The area a pointer crosses on its way from the point where it left a
 * submenu's trigger to the near side of a submenu placed beside the parent
 * menu: the exit point, pulled back a little, and the submenu's corners.
 * Null when a narrow window stacks the submenu above or below instead; the
 * pointer then crosses no sibling diagonally on its way.
 */
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

/** Ray casting: whether `point` lies inside `polygon`. */
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
  /** The pointer left an open submenu's trigger, perhaps towards `submenu`. */
  leave(event: PointerPosition, submenu: Element | null): void;
  /** Whether this move is still on its way to the open submenu. */
  holds(event: PointerPosition): boolean;
  /** The open submenu closed: nothing is on its way anywhere. */
  reset(): void;
}

/**
 * One grace area per parent menu, shared by its submenu triggers, so a
 * diagonal move to an open submenu does not open a sibling on the way.
 */
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
