import { createContext } from "react";

type OpenAt = (anchor: HTMLElement) => void;

/**
 * Lets a right-click in the tree (or the context-menu key on a row) open the
 * Explorer actions menu at that row (ED-18). Each Explorer has its own; its
 * menu registers while mounted.
 */
export interface ExplorerContextMenu {
  register: (openAt: OpenAt) => () => void;
  /** False when no menu is mounted. */
  open: (anchor: HTMLElement) => boolean;
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
    open: (anchor) => {
      if (!current) return false;
      current(anchor);
      return true;
    },
  };
}

export const ExplorerContextMenuContext =
  createContext<ExplorerContextMenu | null>(null);
