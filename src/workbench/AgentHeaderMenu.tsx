import type { ReactNode, RefObject } from "react";
import { FileDiff } from "../ui/leanIcons";
import { MenuPopover } from "../ui/MenuPopover";
import { AgentHeaderMenuContext } from "./AgentToolsProvider";

/** The Agent header "…" menu body; it loads before its first open. */
export function AgentHeaderMenu({
  children,
  close,
  menuId,
  onChanges,
  open,
  openingFocus,
  projectName,
  trigger,
}: {
  children: ReactNode;
  close: () => void;
  menuId: string;
  onChanges: () => void;
  open: boolean;
  openingFocus: "first" | "last";
  projectName: string;
  trigger: RefObject<HTMLButtonElement | null>;
}) {
  return (
    <MenuPopover
      id={menuId}
      open={open}
      triggerRef={trigger}
      ariaLabel="Chat tools"
      className="agent-header-more__menu"
      gap={16}
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
