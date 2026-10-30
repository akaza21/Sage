import type { MouseEvent } from "react";

import { SageMark } from "../ui/leanIconCore";
import { Files, MessageSquare, Search } from "../ui/leanIcons";

import { useWorkbenchStore, type SidebarView } from "../app/store";

// About half a second at 60 Hz.
const BROWSE_FOCUS_FRAMES = 30;

export function EditorWelcome({ onOpenAgent }: { onOpenAgent: () => void }) {
  const setSidebarView = useWorkbenchStore((state) => state.setSidebarView);

  const openView = (view: SidebarView) => setSidebarView(view);
  // Browse files also moves focus into the Explorer, even when it is already
  // open: to the selected row, or else the first row. Explorer mounts on a
  // later render, its chunk may still be loading on a cold start, and its
  // rows wait for the first listing, so this retries for a few frames and
  // settles for any button in the tree on the last one. It gives up if focus
  // moved on meanwhile. The compact drawer focusing its own panel doesn't
  // count as moving on.
  const browseFiles = (event: MouseEvent<HTMLButtonElement>) => {
    const button = event.currentTarget;
    openView("explorer");
    const focusTree = (framesLeft: number) => {
      const active = document.activeElement;
      if (
        active !== button &&
        active !== document.body &&
        active !== null &&
        !active.matches(".sidebar")
      ) {
        return;
      }
      const tree = document.querySelector(
        '[role="tree"][aria-label="Project files"]',
      );
      const target =
        tree?.querySelector<HTMLElement>(
          ".tree__row--selected:not(:disabled)",
        ) ??
        tree?.querySelector<HTMLElement>('[role="treeitem"]:not(:disabled)') ??
        (framesLeft > 0 ? null : tree?.querySelector<HTMLElement>("button"));
      if (target) target.focus();
      else if (framesLeft > 0) {
        requestAnimationFrame(() => focusTree(framesLeft - 1));
      }
    };
    requestAnimationFrame(() => focusTree(BROWSE_FOCUS_FRAMES));
  };

  return (
    <section className="editor-welcome" aria-label="Editor welcome">
      <SageMark className="editor-welcome__mark" />
      <div className="editor-welcome__actions">
        <button type="button" onClick={browseFiles}>
          <Files size={16} aria-hidden="true" />
          Browse files
        </button>
        <button type="button" onClick={() => openView("search")}>
          <Search size={16} aria-hidden="true" />
          Search files
        </button>
        <button type="button" onClick={onOpenAgent}>
          <MessageSquare size={16} aria-hidden="true" />
          Open Agent
        </button>
      </div>
      <p>Use Settings to tune how every File is displayed.</p>
    </section>
  );
}
