import { FileDiff, MoreHorizontal } from "../ui/leanIcons";
import { useId, useRef, useState, type ReactNode } from "react";
import { AgentHeaderMenuContext } from "./AgentToolsProvider";
import { MenuPopover } from "../ui/MenuPopover";
import { lazyParts } from "../features/chat/lazyParts";
import type { AgentSessionChrome } from "../app/agentChatOwner";
import type * as ActionsChunk from "./ProjectChatActionPopover";
const ActionsPart = lazyParts<typeof ActionsChunk>(
  () => import("./ProjectChatActionPopover"),
);
export interface ProjectChatMenuProps {
  chatId: string;
  chatTitle: string;
  pinned: boolean;
  open: boolean;
  disabled: boolean;
  archiveDisabled: boolean;
  projectId: string;
  onOpenChange: (open: boolean) => void;
  onRename: () => void;
  onArchive?: () => void;
  onMarkUnread?: () => void;
  triggerRef?: (node: HTMLButtonElement | null) => void;
  sessionChrome?: AgentSessionChrome | null;
  triggerLabel?: string;
  triggerRole?: "menuitem";
}
export function ProjectChatActionMenu(props: ProjectChatMenuProps) {
  const {
    chatTitle,
    open,
    disabled,
    onOpenChange,
    triggerRef,
    triggerLabel,
    triggerRole,
  } = props;
  const localTriggerRef = useRef<HTMLButtonElement | null>(null);
  const [openingFocus, setOpeningFocus] = useState<"first" | "last" | null>(
    null,
  );
  const menuId = useId();
  return (
    <div className="project-chat-actions">
      <button
        ref={(node) => {
          localTriggerRef.current = node;
          triggerRef?.(node);
        }}
        className="project-chat__actions"
        role={triggerRole}
        type="button"
        aria-label={triggerLabel ?? `Chat actions for ${chatTitle}`}
        aria-controls={menuId}
        aria-haspopup="menu"
        aria-expanded={open}
        disabled={disabled}
        onClick={() => {
          setOpeningFocus("first");
          onOpenChange(!open);
        }}
        onKeyDown={(event) => {
          if (event.key !== "ArrowDown" && event.key !== "ArrowUp") return;
          event.preventDefault();
          setOpeningFocus(event.key === "ArrowUp" ? "last" : "first");
          onOpenChange(true);
        }}
      >
        <MoreHorizontal size={16} aria-hidden="true" />
      </button>
      {open || (props.sessionChrome && openingFocus) ? (
        <ActionsPart
          part="ProjectChatActionPopover"
          {...props}
          localTriggerRef={localTriggerRef}
          menuId={menuId}
          openingFocus={openingFocus ?? "first"}
        />
      ) : null}
    </div>
  );
}

// The Agent header "…" (Codex 68): the project row hosts the chat actions
// above as a nested menu, then Changes opens this Chat's diff picker.
export function AgentHeaderMore({
  scope,
  title,
  projectName,
  disabled,
  onChanges,
  onDismiss,
  children,
}: {
  scope: string;
  title: string;
  projectName: string;
  disabled: boolean;
  onChanges: () => void;
  onDismiss?: () => void;
  children: ReactNode;
}) {
  // Open state belongs to one Chat scope: a Chat switch closes the menu
  // without remounting the trigger, so focus can still return to it.
  const [openScope, setOpenScope] = useState<string | null>(null);
  const open = openScope === scope;
  const setOpen = (next: boolean) => setOpenScope(next ? scope : null);
  const [openingFocus, setOpeningFocus] = useState<"first" | "last">("first");
  const close = () => {
    setOpen(false);
    onDismiss?.();
  };
  if (disabled && open) setOpen(false);
  const trigger = useRef<HTMLButtonElement>(null);
  const menuId = useId();
  return (
    <div className="agent-header-more">
      <button
        ref={trigger}
        className="project-chat__actions"
        type="button"
        aria-label={`Chat actions for ${title} task`}
        aria-controls={menuId}
        aria-haspopup="menu"
        aria-expanded={open}
        disabled={disabled}
        onClick={() => {
          setOpeningFocus("first");
          if (open) close();
          else setOpen(true);
        }}
        onKeyDown={(event) => {
          if (event.key === "ArrowDown" || event.key === "ArrowUp") {
            event.preventDefault();
            setOpeningFocus(event.key === "ArrowUp" ? "last" : "first");
            setOpen(true);
          }
        }}
      >
        <MoreHorizontal size={16} aria-hidden="true" />
      </button>
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
    </div>
  );
}
