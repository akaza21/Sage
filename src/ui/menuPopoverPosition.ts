import { createContext, type CSSProperties, type RefObject } from "react";

export const GUTTER = 8;
const MIN_USABLE_HEIGHT = 120;
export const PopoverPlacementContext = createContext<{
  originRef: RefObject<HTMLElement | null>;
  position: CSSProperties;
} | null>(null);

export type MenuPlacement =
  "below-end" | "below-start" | "above-start" | "above-end";

export function menuPopoverPosition(
  trigger: HTMLElement,
  popover: HTMLElement,
  origin: HTMLElement,
  gap: number,
  matchTriggerWidth: boolean,
  placement: MenuPlacement = "below-end",
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
  const dock = origin.closest(".chat-dock")?.getBoundingClientRect();
  if (dock && dock.width >= width) {
    leftEdge = Math.max(leftEdge, dock.left);
    rightEdge = Math.min(rightEdge, dock.right);
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
  const preferAbove = placement.startsWith("above-");
  const placeAbove = preferAbove
    ? surface.height <= roomAbove || roomAbove > roomBelow
    : surface.height > roomBelow && roomAbove > roomBelow;
  let top = placeAbove
    ? anchor.top - surface.height - gap
    : anchor.bottom + gap;
  // A composer menu that opens upward clears the changed-files summary above
  // the composer instead of covering it.
  const edits =
    dock &&
    origin
      .closest(".chat-surface")
      ?.querySelector(".chat-edit-summary-zone:not(:empty)")
      ?.getBoundingClientRect();
  if (edits && placeAbove && top < edits.bottom)
    top = Math.min(top, edits.top - surface.height - gap);
  return {
    ...shared,
    left: clampLeft(
      placement.endsWith("-end") ? anchor.right - width : anchor.left,
    ),
    top: clampTop(top),
  };
}

const MAX_CONTAINING_BLOCK_DEPTH = 32;

/**
 * A popover portalled into an element with a transform, filter or backdrop
 * filter (a modal dialog, for example) is fixed relative to that element, not
 * the viewport; otherwise its placement and the dialog's layout chase each
 * other.
 */
export function inFixedContainingBlock(
  position: CSSProperties,
  popover: HTMLElement,
): CSSProperties {
  let node = popover.parentElement;
  for (
    let depth = 0;
    node && node !== document.body && depth < MAX_CONTAINING_BLOCK_DEPTH;
    depth += 1, node = node.parentElement
  ) {
    const style = getComputedStyle(node);
    const set = (value: string | undefined) =>
      Boolean(value) && value !== "none";
    if (
      set(style.transform) ||
      set(style.filter) ||
      set(style.backdropFilter) ||
      set(style.perspective) ||
      /paint|layout|strict|content/.test(style.contain ?? "")
    ) {
      const box = node.getBoundingClientRect();
      const x = box.left + node.clientLeft - node.scrollLeft;
      const y = box.top + node.clientTop - node.scrollTop;
      return {
        ...position,
        ...(typeof position.left === "number"
          ? { left: position.left - x }
          : {}),
        ...(typeof position.top === "number" ? { top: position.top - y } : {}),
      };
    }
  }
  return position;
}
