import {
  FolderOpen,
  History,
  Home,
  ListFilter,
  Search,
  Settings,
} from "../ui/leanIcons";
import { useCallback, useId, useRef, useState } from "react";

export function AgentPrimaryNavigation({
  onNewChat,
  onSearch,
  onSettings,
  searchDisabled,
}: {
  onNewChat: () => void;
  onSearch: () => void;
  onSettings: () => void;
  searchDisabled: boolean;
}) {
  const scrollToSection =
    (target: "Recent" | "Projects") =>
    (event: React.MouseEvent<HTMLButtonElement>) => {
      const section =
        target === "Projects"
          ? '[aria-label="Projects"], [aria-label="Chats"]'
          : '[aria-label="Recent"]';
      event.currentTarget
        .closest(".project-sidebar")
        ?.querySelector<HTMLElement>(section)
        ?.scrollIntoView?.();
    };
  return (
    <nav className="project-sidebar__icon-rail" aria-label="Primary navigation">
      <button
        type="button"
        aria-label="New chat"
        title="New chat"
        onClick={onNewChat}
      >
        <Home size={18} aria-hidden="true" />
      </button>
      <button
        type="button"
        aria-label="Go to recent chats"
        title="Recent chats"
        onClick={scrollToSection("Recent")}
      >
        <History size={18} aria-hidden="true" />
      </button>
      <button
        type="button"
        aria-label="Go to projects"
        title="Projects"
        onClick={scrollToSection("Projects")}
      >
        <FolderOpen size={18} aria-hidden="true" />
      </button>
      <button
        type="button"
        aria-label="Search chats"
        title="Search chats"
        disabled={searchDisabled}
        onClick={onSearch}
      >
        <Search size={18} aria-hidden="true" />
      </button>
      <button
        type="button"
        aria-label="Open settings from rail"
        title="Settings"
        onClick={onSettings}
      >
        <Settings size={18} aria-hidden="true" />
      </button>
    </nav>
  );
}

import { lazyParts } from "../features/chat/lazyParts";
import type * as FilterOptionsChunk from "./AgentRailFilterOptions";

/** The open options menu (AgentRailFilterOptions.tsx), loaded on first open. */
const FilterOptionsPart = lazyParts<typeof FilterOptionsChunk>(
  () => import("./AgentRailFilterOptions"),
);

export function AgentRailFilterMenu({
  onMarkAllRead,
}: {
  onMarkAllRead: () => void;
}) {
  const [open, setOpen] = useState(false);
  const [openingFocus, setOpeningFocus] = useState<"first" | "last">("first");
  const triggerRef = useRef<HTMLButtonElement | null>(null);
  const menuId = useId();

  const closeAndRestoreFocus = useCallback(() => {
    setOpen(false);
    queueMicrotask(() => triggerRef.current?.focus());
  }, []);

  return (
    <div className="agent-rail-filter">
      <button
        ref={triggerRef}
        className="agent-rail-filter__trigger"
        type="button"
        aria-label="Chat list options"
        aria-haspopup="menu"
        aria-expanded={open}
        aria-controls={menuId}
        title="Chat list options"
        onClick={() => {
          setOpeningFocus("first");
          setOpen((value) => !value);
        }}
        onKeyDown={(event) => {
          if (event.key !== "ArrowDown" && event.key !== "ArrowUp") return;
          event.preventDefault();
          setOpeningFocus(event.key === "ArrowUp" ? "last" : "first");
          setOpen(true);
        }}
      >
        <ListFilter size={14} aria-hidden="true" />
      </button>
      {open ? (
        <FilterOptionsPart
          part="AgentRailFilterPopover"
          menuId={menuId}
          openingFocus={openingFocus}
          triggerRef={triggerRef}
          onDismiss={() => setOpen(false)}
          onCloseAndRestoreFocus={closeAndRestoreFocus}
          onMarkAllRead={onMarkAllRead}
        />
      ) : null}
    </div>
  );
}
