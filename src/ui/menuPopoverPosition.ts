import { createContext, type CSSProperties, type RefObject } from "react";

const GUTTER = 8;
export const PopoverPlacementContext = createContext<{
  originRef: RefObject<HTMLElement | null>;
  position: CSSProperties;
} | null>(null);

export function menuPopoverPosition(
  trigger: HTMLElement,
  popover: HTMLElement,
  origin: HTMLElement,
  gap: number,
  matchTriggerWidth: boolean,
): CSSProperties {
  const anchor = trigger.getBoundingClientRect();
  const surface = popover.getBoundingClientRect();
  const width = Math.max(surface.width, matchTriggerWidth ? anchor.width : 0);
  let leftEdge = GUTTER;
  let rightEdge = window.innerWidth - GUTTER;
  for (const selector of [".composer", ".agent-chat-pane"]) {
    const owner = origin.closest(selector)?.getBoundingClientRect();
    if (!owner) continue;
    const left = Math.max(GUTTER, owner.left);
    const right = Math.min(rightEdge, owner.right);
    if (right - left >= width) {
      leftEdge = left;
      rightEdge = right;
      break;
    }
  }
  const clampLeft = (left: number) =>
    Math.min(Math.max(leftEdge, left), Math.max(leftEdge, rightEdge - width));
  const clampTop = (top: number) =>
    Math.max(
      GUTTER,
      Math.min(top, window.innerHeight - surface.height - GUTTER),
    );
  const parent = trigger.closest(".menu-popover")?.getBoundingClientRect();
  const shared = {
    minWidth: matchTriggerWidth ? anchor.width : undefined,
    visibility: "visible" as const,
  };
  if (parent) {
    const right = parent.right + gap;
    const left = parent.left - width - gap;
    if (right + width <= rightEdge || left >= leftEdge) {
      return {
        ...shared,
        left: right + width <= rightEdge ? right : left,
        top: clampTop(anchor.top),
      };
    }
    const below = window.innerHeight - parent.bottom - gap - GUTTER;
    const maxHeight = Math.max(0, Math.min(320, below));
    return {
      ...shared,
      left: clampLeft(anchor.left),
      top: parent.bottom + gap,
      maxHeight,
    };
  }
  const roomBelow = window.innerHeight - anchor.bottom - GUTTER;
  const roomAbove = anchor.top - GUTTER;
  const placeAbove = surface.height > roomBelow && roomAbove > roomBelow;
  return {
    ...shared,
    left: clampLeft(anchor.right - width),
    top: clampTop(
      placeAbove ? anchor.top - surface.height - gap : anchor.bottom + gap,
    ),
  };
}
