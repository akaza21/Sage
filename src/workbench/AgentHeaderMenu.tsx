import { useState, type ReactNode, type RefObject } from "react";
import { FileDiff } from "../ui/leanIcons";
import { MenuPopover } from "../ui/MenuPopover";
import { AgentHeaderMenuContext } from "./AgentToolsProvider";

export function AgentHeaderMenu({
  children,
  close,
  focusMoved,
  menuId,
  onChanges,
  open,
  openingFocus,
  projectName,
  trigger,
}: {
  children: ReactNode;
  close: () => void;
  focusMoved: () => boolean;
  menuId: string;
  onChanges: () => void;
  open: boolean;
  openingFocus: "first" | "last";
  projectName: string;
  trigger: RefObject<HTMLButtonElement | null>;
}) {
  const [holdFocus, setHoldFocus] = useState(focusMoved);
  if (!open && holdFocus) setHoldFocus(false);
  return (
    <MenuPopover
      id={menuId}
      open={open}
      triggerRef={trigger}
      ariaLabel="Chat tools"
      className="agent-header-more__menu"
      gap={16}
      holdFocus={holdFocus}
      initialFocus={openingFocus}
      keepMounted
      onDismiss={close}
    >
      <div className="agent-header-more__project">
        <span title={projectName}>{projectName}</span>
        <AgentHeaderMenuContext
          value={{ close, returnFocus: () => trigger.current?.focus() }}
        >
          {children}
        </AgentHeaderMenuContext>
      </div>
      <button
        type="button"
        className="menu-item"
        role="menuitem"
        onClick={() => {
          close();
          trigger.current?.focus();
          onChanges();
        }}
      >
        <FileDiff size={14} aria-hidden="true" />
        <span>Changes</span>
      </button>
    </MenuPopover>
  );
}
