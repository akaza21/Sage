import { useId, useRef, useState, type ReactNode } from "react";
import type { AgentSessionChrome } from "../app/agentChatOwner";
import { lazyParts } from "../features/chat/lazyParts";
import { FileDiff, MoreHorizontal } from "../ui/leanIcons";
import { MenuPopover } from "../ui/MenuPopover";
import { AgentHeaderMenuContext } from "./AgentToolsProvider";
import type { AgentRailTrailingState } from "./agentRailStatusView";
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

// The Agent header "…": the project row hosts the chat actions
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

/**
 * A rail row's trailing state. Running and unread rows show only a glyph
 * (spinner, dot); a failed or waiting row that is open keeps its label.
 */
export function ProjectChatTrailingState({
  awaitingApproval,
  focused,
  state,
}: {
  awaitingApproval: boolean;
  focused: boolean;
  state: AgentRailTrailingState | null;
}) {
  if (awaitingApproval)
    return (
      <span
        className="project-chat__state project-chat__approval-spinner"
        aria-hidden="true"
      >
        <span className="spinner agent-chat-chip__spinner" />
      </span>
    );
  if (focused && (state?.kind === "failed" || state?.kind === "waiting"))
    return (
      <span
        className={`project-chat__state project-chat__status-chip project-chat__status-chip--${state.kind}`}
      >
        {state.label}
      </span>
    );
  return <ProjectChatRailGlyph state={state} />;
}

const GLYPH_LABELS = {
  unread: "Unread updates",
  working: "Working",
  recovering: "Recovering",
  waiting: "Needs attention",
} as const;

export function ProjectChatRailGlyph({
  state,
}: {
  state: AgentRailTrailingState | null;
}) {
  if (!state) return null;
  if (state.kind === "failed")
    return (
      <span className="project-chat__state project-chat__state--failed">
        {state.label}
      </span>
    );
  return (
    <span
      className={`project-chat__state project-chat__state--${state.kind}`}
      role="img"
      aria-label={GLYPH_LABELS[state.kind]}
    >
      {state.kind === "working" || state.kind === "recovering" ? (
        <span className="project-chat__working" aria-hidden="true" />
      ) : null}
    </span>
  );
}
