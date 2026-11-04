import { createContext } from "react";

import type { PopoverPoint } from "../../ui/pointerPopoverPosition";

type OpenAt = (anchor: HTMLElement, point?: PopoverPoint) => void;

/**
 * Lets a right-click in the tree (or the context-menu key on a row) open the
 * Explorer actions menu for that row (ED-18): at the pointer for a click, or
 * below the row for the keyboard. Each Explorer has its own; its menu
 * registers while mounted.
 */
export interface ExplorerContextMenu {
  register: (openAt: OpenAt) => () => void;
  /** False when no menu is mounted. */
  open: (anchor: HTMLElement, point?: PopoverPoint) => boolean;
}

export function createExplorerContextMenu(): ExplorerContextMenu {
  let current: OpenAt | null = null;
  return {
    register: (openAt) => {
      current = openAt;
      return () => {
        if (current === openAt) current = null;
      };
    },
    open: (anchor, point) => {
      if (!current) return false;
      current(anchor, point);
      return true;
    },
  };
}

export const ExplorerContextMenuContext =
  createContext<ExplorerContextMenu | null>(null);

/**
 * The keyboard's own way to ask for the menu. WebKit on macOS sends no
 * contextmenu event for it, so rows listen for the keys too.
 */
export const isContextMenuKey = (event: { key: string; shiftKey: boolean }) =>
  event.key === "ContextMenu" || (event.key === "F10" && event.shiftKey);
