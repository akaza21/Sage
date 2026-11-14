import { useState, type ComponentType, type RefObject } from "react";

import {
  ArrowDownUp,
  Check,
  FolderOpen,
  List,
  PanelLeft,
} from "../ui/leanIcons";
import { MenuPopover } from "../ui/MenuPopover";
import { MenuSubmenu } from "../ui/MenuSubmenu";
import { useSubmenuGrace } from "../ui/submenuGrace";
import type {
  AgentRailOrdering,
  AgentRailOrganizeMode,
} from "./agentRailStatusView";
import {
  setAgentRailOrdering,
  setAgentRailOrganize,
  useAgentRailViewPrefs,
} from "./agentRailViewPrefs";

type IconComponent = ComponentType<{ size?: number; "aria-hidden"?: "true" }>;

interface Choice<T extends string> {
  value: T;
  label: string;
  icon?: IconComponent;
}

const ORGANIZE_CHOICES: ReadonlyArray<Choice<AgentRailOrganizeMode>> = [
  { value: "project", label: "By project", icon: FolderOpen },
  { value: "list", label: "In one list", icon: List },
];

const ORDERING_CHOICES: ReadonlyArray<Choice<AgentRailOrdering>> = [
  { value: "recency", label: "Last updated" },
  { value: "manual", label: "Manual order" },
];

type Submenu = "organize" | "ordering";

export function AgentRailOptionsPopover({
  menuId,
  openingFocus,
  triggerRef,
  onDismiss,
  onCloseAndRestoreFocus,
}: {
  menuId: string;
  openingFocus: "first" | "last";
  triggerRef: RefObject<HTMLButtonElement | null>;
  onDismiss: () => void;
  onCloseAndRestoreFocus: () => void;
}) {
  const prefs = useAgentRailViewPrefs();
  const [submenu, setSubmenu] = useState<Submenu | null>(null);
  const grace = useSubmenuGrace();
  return (
    <MenuPopover
      id={menuId}
      open
      ariaLabel="Sidebar options"
      className="agent-rail-options__menu"
      initialFocus={openingFocus}
      placement="below-start"
      triggerRef={triggerRef}
      onDismiss={onDismiss}
    >
      <MenuSubmenu
        label="Organize sidebar"
        leading={<PanelLeft size={14} aria-hidden="true" />}
        className={ITEM_CLASS}
        menuClassName={SUBMENU_CLASS}
        open={submenu === "organize"}
        onOpenChange={(open) => setSubmenu(open ? "organize" : null)}
        grace={grace}
      >
        <Choices
          choices={ORGANIZE_CHOICES}
          selected={prefs.organize}
          onSelect={(value) => {
            setAgentRailOrganize(value);
            onCloseAndRestoreFocus();
          }}
        />
      </MenuSubmenu>
      <MenuSubmenu
        label="Sort chats by"
        leading={<ArrowDownUp size={14} aria-hidden="true" />}
        className={ITEM_CLASS}
        menuClassName={SUBMENU_CLASS}
        open={submenu === "ordering"}
        onOpenChange={(open) => setSubmenu(open ? "ordering" : null)}
        grace={grace}
      >
        <Choices
          choices={ORDERING_CHOICES}
          selected={prefs.ordering}
          onSelect={(value) => {
            setAgentRailOrdering(value);
            onCloseAndRestoreFocus();
          }}
        />
      </MenuSubmenu>
    </MenuPopover>
  );
}

const ITEM_CLASS = "menu-item agent-rail-options__item";
const SUBMENU_CLASS = "agent-rail-options__menu agent-rail-options__submenu";

function Choices<T extends string>({
  choices,
  selected,
  onSelect,
}: {
  choices: ReadonlyArray<Choice<T>>;
  selected: T;
  onSelect: (value: T) => void;
}) {
  return choices.map(({ value, label, icon: ChoiceIcon }) => (
    <button
      key={value}
      className={ITEM_CLASS}
      type="button"
      role="menuitemradio"
      tabIndex={-1}
      aria-checked={selected === value}
      onClick={() => onSelect(value)}
    >
      {ChoiceIcon ? <ChoiceIcon size={14} aria-hidden="true" /> : null}
      <span>{label}</span>
      {selected === value ? (
        <Check
          className="agent-rail-options__trailing"
          size={14}
          aria-hidden="true"
        />
      ) : null}
    </button>
  ));
}
