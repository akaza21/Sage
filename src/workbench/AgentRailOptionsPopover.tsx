import {
  useId,
  useRef,
  useState,
  type ComponentType,
  type RefObject,
} from "react";

import {
  ArrowDownUp,
  Check,
  ChevronRight,
  FolderOpen,
  List,
  PanelLeft,
} from "../ui/leanIcons";
import { MenuPopover } from "../ui/MenuPopover";
import type {
  AgentRailOrdering,
  AgentRailOrganizeMode,
} from "./agentRailStatusView";
import {
  setAgentRailOrdering,
  setAgentRailOrganize,
  useAgentRailViewPrefs,
} from "./agentRailViewPrefs";

/**
 * The open sidebar options menu. A lazy chunk: it loads on the first open,
 * so the rail's eager path carries only its trigger.
 */

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
      <SubmenuItem
        label="Organize sidebar"
        icon={PanelLeft}
        open={submenu === "organize"}
        onOpenChange={(open) => setSubmenu(open ? "organize" : null)}
        choices={ORGANIZE_CHOICES}
        selected={prefs.organize}
        onSelect={(value) => {
          setAgentRailOrganize(value);
          onCloseAndRestoreFocus();
        }}
      />
      <SubmenuItem
        label="Sort chats by"
        icon={ArrowDownUp}
        open={submenu === "ordering"}
        onOpenChange={(open) => setSubmenu(open ? "ordering" : null)}
        choices={ORDERING_CHOICES}
        selected={prefs.ordering}
        onSelect={(value) => {
          setAgentRailOrdering(value);
          onCloseAndRestoreFocus();
        }}
      />
    </MenuPopover>
  );
}

function SubmenuItem<T extends string>({
  label,
  icon: Icon,
  open,
  onOpenChange,
  choices,
  selected,
  onSelect,
}: {
  label: string;
  icon: IconComponent;
  open: boolean;
  onOpenChange: (open: boolean) => void;
  choices: ReadonlyArray<Choice<T>>;
  selected: T;
  onSelect: (value: T) => void;
}) {
  const triggerRef = useRef<HTMLButtonElement>(null);
  const submenuId = useId();
  const close = () => {
    onOpenChange(false);
    triggerRef.current?.focus();
  };
  return (
    <>
      <button
        ref={triggerRef}
        className="menu-item agent-rail-options__item"
        type="button"
        role="menuitem"
        tabIndex={-1}
        aria-haspopup="menu"
        aria-expanded={open}
        aria-controls={open ? submenuId : undefined}
        onPointerEnter={() => onOpenChange(true)}
        onClick={() => onOpenChange(true)}
        onKeyDown={(event) => {
          if (event.key !== "ArrowRight") return;
          event.preventDefault();
          event.stopPropagation();
          onOpenChange(true);
        }}
      >
        <Icon size={14} aria-hidden="true" />
        <span>{label}</span>
        <ChevronRight
          className="agent-rail-options__trailing"
          size={14}
          aria-hidden="true"
        />
      </button>
      <MenuPopover
        id={submenuId}
        open={open}
        ariaLabel={label}
        className="agent-rail-options__menu agent-rail-options__submenu"
        initialFocus="selected"
        portalTarget="closest-popover"
        triggerRef={triggerRef}
        onDismiss={(reason) => {
          if (reason === "escape") close();
          else onOpenChange(false);
        }}
      >
        <div
          className="agent-rail-options__choices"
          onKeyDown={(event) => {
            if (event.key !== "ArrowLeft") return;
            event.preventDefault();
            event.stopPropagation();
            close();
          }}
        >
          {choices.map(({ value, label: choiceLabel, icon: ChoiceIcon }) => (
            <button
              key={value}
              className="menu-item agent-rail-options__item"
              type="button"
              role="menuitemradio"
              tabIndex={-1}
              aria-checked={selected === value}
              onClick={() => onSelect(value)}
            >
              {ChoiceIcon ? <ChoiceIcon size={14} aria-hidden="true" /> : null}
              <span>{choiceLabel}</span>
              {selected === value ? (
                <Check
                  className="agent-rail-options__trailing"
                  size={14}
                  aria-hidden="true"
                />
              ) : null}
            </button>
          ))}
        </div>
      </MenuPopover>
    </>
  );
}
