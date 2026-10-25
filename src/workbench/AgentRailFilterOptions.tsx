import type { RefObject } from "react";

import { MenuPopover } from "../ui/MenuPopover";
import {
  AGENT_RAIL_GROUPINGS,
  AGENT_RAIL_ORDERINGS,
  AGENT_RAIL_SHOW_FIELDS,
  AGENT_RAIL_STATUS_LABELS,
  AGENT_RAIL_STATUS_SECTIONS,
} from "./agentRailStatusView";
import {
  collapseAllAgentRailSections,
  setAgentRailGrouping,
  setAgentRailOrdering,
  setAgentRailShowArchived,
  setAgentRailShowField,
  setAgentRailStatusFilter,
  useAgentRailViewPrefs,
} from "./agentRailViewPrefs";

/**
 * The open "Chat list options" menu. A lazy chunk: it loads on the first
 * open, so the rail's eager path carries only its trigger.
 */

const AGENT_RAIL_COLLAPSIBLE_SECTIONS = [
  "pinned",
  "projects",
  "recent",
  ...AGENT_RAIL_STATUS_SECTIONS,
] as const;

const groupingLabels = { folder: "Folder", status: "Status" } as const;
const orderingLabels = { recency: "Recency", title: "Title" } as const;
const showLabels = {
  updated: "Updated",
  workspace: "Workspace",
  branch: "Branch",
} as const;

export function AgentRailFilterPopover({
  menuId,
  openingFocus,
  triggerRef,
  onDismiss,
  onCloseAndRestoreFocus,
  onMarkAllRead,
}: {
  menuId: string;
  openingFocus: "first" | "last";
  triggerRef: RefObject<HTMLButtonElement | null>;
  onDismiss: () => void;
  onCloseAndRestoreFocus: () => void;
  onMarkAllRead: () => void;
}) {
  const prefs = useAgentRailViewPrefs();
  return (
    <MenuPopover
      id={menuId}
      open
      ariaLabel="Chat list options"
      className="agent-rail-filter__menu"
      initialFocus={openingFocus}
      trapFocus
      triggerRef={triggerRef}
      onDismiss={onDismiss}
    >
      <div
        className="menu-group"
        role="group"
        aria-labelledby={`${menuId}-grouping`}
      >
        <p id={`${menuId}-grouping`} className="menu-group-label">
          Grouping
        </p>
        {AGENT_RAIL_GROUPINGS.map((grouping) => (
          <button
            className="menu-item"
            key={grouping}
            type="button"
            role="menuitemradio"
            tabIndex={-1}
            aria-checked={prefs.grouping === grouping}
            onClick={() => setAgentRailGrouping(grouping)}
          >
            {groupingLabels[grouping]}
          </button>
        ))}
      </div>
      <div
        className="menu-group"
        role="group"
        aria-labelledby={`${menuId}-ordering`}
      >
        <p id={`${menuId}-ordering`} className="menu-group-label">
          Ordering
        </p>
        {AGENT_RAIL_ORDERINGS.map((ordering) => (
          <button
            className="menu-item"
            key={ordering}
            type="button"
            role="menuitemradio"
            tabIndex={-1}
            aria-checked={prefs.ordering === ordering}
            onClick={() => setAgentRailOrdering(ordering)}
          >
            {orderingLabels[ordering]}
          </button>
        ))}
      </div>
      <div
        className="menu-group"
        role="group"
        aria-labelledby={`${menuId}-show`}
      >
        <p id={`${menuId}-show`} className="menu-group-label">
          Show
        </p>
        {AGENT_RAIL_SHOW_FIELDS.map((field) => (
          <button
            className="menu-item"
            key={field}
            type="button"
            role="menuitemcheckbox"
            tabIndex={-1}
            aria-checked={prefs.show[field]}
            onClick={() => setAgentRailShowField(field, !prefs.show[field])}
          >
            {showLabels[field]}
          </button>
        ))}
      </div>
      <div
        className="menu-group"
        role="group"
        aria-labelledby={`${menuId}-filters`}
      >
        <p id={`${menuId}-filters`} className="menu-group-label">
          Filters
        </p>
        {(["all", ...AGENT_RAIL_STATUS_SECTIONS] as const).map((status) => (
          <button
            key={status}
            className="menu-item"
            type="button"
            role="menuitemradio"
            tabIndex={-1}
            aria-checked={prefs.statusFilter === status}
            onClick={() => setAgentRailStatusFilter(status)}
          >
            {status === "all"
              ? "All statuses"
              : AGENT_RAIL_STATUS_LABELS[status]}
          </button>
        ))}
      </div>
      <button
        className="menu-item"
        type="button"
        role="menuitemcheckbox"
        tabIndex={-1}
        aria-checked={prefs.showArchived}
        onClick={() => setAgentRailShowArchived(!prefs.showArchived)}
      >
        Archived
      </button>
      <button
        className="menu-item"
        type="button"
        role="menuitem"
        tabIndex={-1}
        onClick={() => {
          collapseAllAgentRailSections(AGENT_RAIL_COLLAPSIBLE_SECTIONS);
          onCloseAndRestoreFocus();
        }}
      >
        Collapse All
      </button>
      <button
        className="menu-item"
        type="button"
        role="menuitem"
        tabIndex={-1}
        onClick={() => {
          onMarkAllRead();
          onCloseAndRestoreFocus();
        }}
      >
        Mark All as Read
      </button>
    </MenuPopover>
  );
}
