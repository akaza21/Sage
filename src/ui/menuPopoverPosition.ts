import { createContext, type CSSProperties, type RefObject } from "react";

const GUTTER = 8;
const MIN_USABLE_HEIGHT = 120;
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
  let leftEdge = GUTTER;
  let rightEdge = window.innerWidth - GUTTER;
  const pane = origin
    .closest(".agent-chat-pane, .agent-chat-center")
    ?.getBoundingClientRect();
  if (pane) {
    leftEdge = Math.max(leftEdge, pane.left);
    rightEdge = Math.min(rightEdge, pane.right);
  }
  const maxWidth = Math.max(0, rightEdge - leftEdge);
  const width = Math.min(
    maxWidth,
    Math.max(surface.width, matchTriggerWidth ? anchor.width : 0),
  );
  const composer = origin.closest(".composer")?.getBoundingClientRect();
  if (composer && composer.width >= width) {
    leftEdge = Math.max(leftEdge, composer.left);
    rightEdge = Math.min(rightEdge, composer.right);
  }
  const clampLeft = (left: number) =>
    Math.min(Math.max(leftEdge, left), Math.max(leftEdge, rightEdge - width));
  let topEdge = GUTTER;
  let bottomEdge = window.innerHeight - GUTTER;
  if (pane) {
    const top = Math.max(topEdge, pane.top);
    const bottom = Math.min(bottomEdge, pane.bottom);
    if (bottom - top >= surface.height) {
      topEdge = top;
      bottomEdge = bottom;
    }
  }
  const clampTop = (top: number, height = surface.height) =>
    Math.max(topEdge, Math.min(top, bottomEdge - height));
  const parent = trigger.closest(".menu-popover")?.getBoundingClientRect();
  const shared = {
    minWidth: matchTriggerWidth ? Math.min(anchor.width, maxWidth) : undefined,
    maxWidth,
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
    const below = bottomEdge - parent.bottom - gap;
    const above = parent.top - gap - topEdge;
    const placeAbove =
      above >= MIN_USABLE_HEIGHT &&
      (below < MIN_USABLE_HEIGHT || above > below);
    const available = placeAbove ? above : below;
    const fallback = available < MIN_USABLE_HEIGHT;
    if (fallback) {
      topEdge = GUTTER;
      bottomEdge = window.innerHeight - GUTTER;
    }
    const maxHeight = Math.max(
      0,
      Math.min(320, fallback ? bottomEdge - topEdge : available),
    );
    return {
      ...shared,
      left: clampLeft(anchor.left),
      top: fallback
        ? clampTop(parent.top, maxHeight)
        : placeAbove
          ? parent.top - gap - Math.min(surface.height, maxHeight)
          : parent.bottom + gap,
      maxHeight,
    };
  }
  const roomBelow = window.innerHeight - anchor.bottom - gap - GUTTER;
  const roomAbove = anchor.top - gap - GUTTER;
  const placeAbove = surface.height > roomBelow && roomAbove > roomBelow;
  return {
    ...shared,
    left: clampLeft(anchor.right - width),
    top: clampTop(
      placeAbove ? anchor.top - surface.height - gap : anchor.bottom + gap,
    ),
  };
}
