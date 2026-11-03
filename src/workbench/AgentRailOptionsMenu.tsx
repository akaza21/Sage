import {
  FolderOpen,
  History,
  Home,
  MoreHorizontal,
  Search,
  Settings,
} from "../ui/leanIcons";
import { useCallback, useContext, useId, useRef, useState } from "react";
import { expandAgentRailSection } from "./agentRailViewPrefs";
import {
  agentRailSectionSelectors,
  AgentHomeNavigationContext,
} from "./agentRailNavigation";

export function AgentPrimaryNavigation({
  onSearch,
  onSettings,
  searchDisabled,
  onNavigate,
}: {
  onSearch?: () => void;
  onSettings?: () => void;
  searchDisabled?: boolean;
  onNavigate?: (target: "Recent" | "Projects") => void;
}) {
  const navigateHome = useContext(AgentHomeNavigationContext);
  const scrollToSection =
    (target: "Recent" | "Projects") =>
    (event: React.MouseEvent<HTMLButtonElement>) => {
      expandAgentRailSection(target === "Projects" ? "projects" : "recent");
      if (onNavigate) return onNavigate(target);
      event.currentTarget
        .closest(".project-sidebar")
        ?.querySelector<HTMLElement>(agentRailSectionSelectors[target])
        ?.scrollIntoView?.();
    };
  return (
    <nav className="project-sidebar__icon-rail" aria-label="Primary navigation">
      <button
        type="button"
        aria-label="Home"
        title="Home"
        onClick={navigateHome}
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
      {onSearch ? (
        <button
          type="button"
          aria-label="Search chats"
          title="Search chats"
          disabled={searchDisabled}
          onClick={onSearch}
        >
          <Search size={18} aria-hidden="true" />
        </button>
      ) : null}
      {onSettings ? (
        <button
          type="button"
          aria-label="Open settings from rail"
          title="Settings"
          onClick={onSettings}
        >
          <Settings size={18} aria-hidden="true" />
        </button>
      ) : null}
    </nav>
  );
}

import { lazyParts } from "../features/chat/lazyParts";
import type * as OptionsPopoverChunk from "./AgentRailOptionsPopover";

/** The open options menu (AgentRailOptionsPopover.tsx), loaded on first open. */
const OptionsPopoverPart = lazyParts<typeof OptionsPopoverChunk>(
  () => import("./AgentRailOptionsPopover"),
);

export function AgentRailOptionsMenu() {
  const [open, setOpen] = useState(false);
  const [openingFocus, setOpeningFocus] = useState<"first" | "last">("first");
  const triggerRef = useRef<HTMLButtonElement | null>(null);
  const menuId = useId();

  const closeAndRestoreFocus = useCallback(() => {
    setOpen(false);
    queueMicrotask(() => triggerRef.current?.focus());
    // Reorganizing the sidebar remounts this trigger under another section,
    // so focus follows the new one.
    requestAnimationFrame(() => {
      if (triggerRef.current?.isConnected) return;
      document
        .querySelector<HTMLElement>(
          ".project-sidebar .agent-rail-options__trigger",
        )
        ?.focus();
    });
  }, []);

  return (
    <div className="agent-rail-options">
      <button
        ref={triggerRef}
        className="agent-rail-options__trigger"
        type="button"
        aria-label="Sidebar options"
        aria-haspopup="menu"
        aria-expanded={open}
        aria-controls={open ? menuId : undefined}
        title="Sidebar options"
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
        <MoreHorizontal size={14} aria-hidden="true" />
      </button>
      {open ? (
        <OptionsPopoverPart
          part="AgentRailOptionsPopover"
          menuId={menuId}
          openingFocus={openingFocus}
          triggerRef={triggerRef}
          onDismiss={() => setOpen(false)}
          onCloseAndRestoreFocus={closeAndRestoreFocus}
        />
      ) : null}
    </div>
  );
}
