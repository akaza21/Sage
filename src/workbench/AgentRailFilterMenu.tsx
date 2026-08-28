import { ListFilter } from "lucide-react";
import { useEffect, useId, useRef, useState } from "react";

import {
  AGENT_RAIL_GROUPINGS,
  AGENT_RAIL_ORDERINGS,
  AGENT_RAIL_SHOW_FIELDS,
  AGENT_RAIL_STATUS_LABELS,
  AGENT_RAIL_STATUS_SECTIONS,
  type AgentRailStatusFilter,
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

const groupingLabels = { folder: "Folder", status: "Status" } as const;
const orderingLabels = { recency: "Recency", title: "Title" } as const;
const showLabels = {
  updated: "Updated",
  workspace: "Workspace",
  branch: "Branch",
} as const;

export function AgentRailFilterMenu({
  onMarkAllRead,
}: {
  onMarkAllRead: () => void;
}) {
  const prefs = useAgentRailViewPrefs();
  const [open, setOpen] = useState(false);
  const rootRef = useRef<HTMLDivElement | null>(null);
  const menuId = useId();

  useEffect(() => {
    if (!open) return;
    const onPointer = (event: PointerEvent) => {
      if (!rootRef.current?.contains(event.target as Node)) setOpen(false);
    };
    window.addEventListener("pointerdown", onPointer);
    return () => window.removeEventListener("pointerdown", onPointer);
  }, [open]);

  return (
    <div className="agent-rail-filter" ref={rootRef}>
      <button
        className="agent-rail-filter__trigger"
        type="button"
        aria-label="Chat list options"
        aria-expanded={open}
        aria-controls={menuId}
        title="Chat list options"
        onClick={() => setOpen((value) => !value)}
      >
        <ListFilter size={14} aria-hidden="true" />
      </button>
      {open ? (
        <div
          className="agent-rail-filter__menu"
          id={menuId}
          role="menu"
          aria-label="Chat list options"
        >
          <p className="agent-rail-filter__heading">Grouping</p>
          {AGENT_RAIL_GROUPINGS.map((grouping) => (
            <button
              key={grouping}
              type="button"
              role="menuitemradio"
              aria-checked={prefs.grouping === grouping}
              onClick={() => setAgentRailGrouping(grouping)}
            >
              {groupingLabels[grouping]}
            </button>
          ))}
          <p className="agent-rail-filter__heading">Ordering</p>
          {AGENT_RAIL_ORDERINGS.map((ordering) => (
            <button
              key={ordering}
              type="button"
              role="menuitemradio"
              aria-checked={prefs.ordering === ordering}
              onClick={() => setAgentRailOrdering(ordering)}
            >
              {orderingLabels[ordering]}
            </button>
          ))}
          <p className="agent-rail-filter__heading">Show</p>
          {AGENT_RAIL_SHOW_FIELDS.map((field) => (
            <button
              key={field}
              type="button"
              role="menuitemcheckbox"
              aria-checked={prefs.show[field]}
              onClick={() => setAgentRailShowField(field, !prefs.show[field])}
            >
              {showLabels[field]}
            </button>
          ))}
          <p className="agent-rail-filter__heading">Filters</p>
          <label className="agent-rail-filter__select">
            Status
            <select
              aria-label="Filter by status"
              value={prefs.statusFilter}
              onChange={(event) =>
                setAgentRailStatusFilter(
                  event.currentTarget.value as AgentRailStatusFilter,
                )
              }
            >
              <option value="all">All</option>
              {AGENT_RAIL_STATUS_SECTIONS.map((id) => (
                <option key={id} value={id}>
                  {AGENT_RAIL_STATUS_LABELS[id]}
                </option>
              ))}
            </select>
          </label>
          <button
            type="button"
            role="menuitemcheckbox"
            aria-checked={prefs.showArchived}
            onClick={() => setAgentRailShowArchived(!prefs.showArchived)}
          >
            Archived
          </button>
          <button
            type="button"
            role="menuitem"
            onClick={() => {
              collapseAllAgentRailSections(AGENT_RAIL_STATUS_SECTIONS);
              setOpen(false);
            }}
          >
            Collapse All
          </button>
          <button
            type="button"
            role="menuitem"
            onClick={() => {
              onMarkAllRead();
              setOpen(false);
            }}
          >
            Mark All as Read
          </button>
        </div>
      ) : null}
    </div>
  );
}
