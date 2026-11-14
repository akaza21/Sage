import { createContext } from "react";

import type { PopoverPoint } from "../../ui/pointerPopoverPosition";

type OpenAt = (anchor: HTMLElement, point?: PopoverPoint) => void;

export interface ExplorerContextMenu {
  register: (openAt: OpenAt) => () => void;
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
 * WebKit on macOS sends no contextmenu event for the context-menu key, so rows
 * listen for the keys too.
 */
export const isContextMenuKey = (event: { key: string; shiftKey: boolean }) =>
  event.key === "ContextMenu" || (event.key === "F10" && event.shiftKey);
