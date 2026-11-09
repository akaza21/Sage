import {
  useCallback,
  useEffect,
  useId,
  useRef,
  useState,
  useSyncExternalStore,
  type ReactNode,
} from "react";
import { createPortal } from "react-dom";
import type { AgentSessionChrome } from "../app/agentChatOwner";
import {
  lazyParts,
  preloadChatHandoffControl,
} from "../features/chat/lazyParts";
import { useColdOpen } from "../ui/coldOpen";
import { Folder, MoreHorizontal } from "../ui/leanIcons";
import type { MenuDismissReason } from "../ui/MenuPopover";
import { ResourceStateNotice } from "../ui/ResourceStateNotice";
import { whenIdle } from "../ui/whenIdle";
import { agentRailHoverAge } from "./agentRailInteractions";
import type { AgentRailTrailingState } from "./agentRailStatusView";
import type * as ActionsChunk from "./ProjectChatActionPopover";
import {
  AgentHeaderMenuPart as MenuPart,
  agentHeaderMenuReady,
  preloadAgentHeaderMenu,
  subscribeAgentHeaderMenu,
} from "./agentTaskHeaderLazy";
// The menu's Hand off item loads with it, so it renders in the menu's
// first commit. A failed Hand off chunk leaves the rest of the menu working.
const ActionsPart = lazyParts<typeof ActionsChunk>(() =>
  Promise.all([
    import("./ProjectChatActionPopover"),
    preloadChatHandoffControl().catch(() => undefined),
  ]).then(([chunk]) => chunk),
);
const preloadMenu = () => void preloadAgentHeaderMenu().catch(() => undefined);
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
  const [openingFocus, setOpeningFocus] = useState<"first" | "last">("first");
  const [loadFailed, setLoadFailed] = useState(false);
  if (open && loadFailed) setLoadFailed(false);
  const close = useCallback(() => {
    setOpenScope(null);
    onDismiss?.();
  }, [onDismiss]);
  if (disabled && open) setOpenScope(null);
  const trigger = useRef<HTMLButtonElement>(null);
  const menuId = useId();
  // The menu body loads once the window is idle, or on the trigger's first
  // hover, focus or press, and then stays mounted like any kept menu. It
  // mounts only once loaded; an open before then waits for it here.
  const loaded = useSyncExternalStore(
    subscribeAgentHeaderMenu,
    agentHeaderMenuReady,
  );
  useEffect(
    () => (agentHeaderMenuReady() ? undefined : whenIdle(preloadMenu)),
    [],
  );
  useEffect(() => {
    if (!open || loaded) return;
    let live = true;
    void preloadAgentHeaderMenu().catch(() => {
      if (!live) return;
      setLoadFailed(true);
      close();
    });
    return () => {
      live = false;
    };
  }, [close, loaded, open]);
  const dismissCold = useCallback(
    (reason: MenuDismissReason) => {
      close();
      if (reason === "escape") trigger.current?.focus();
    },
    [close],
  );
  const coldOpen = useColdOpen({
    dismiss: dismissCold,
    loaded,
    open,
    triggerRef: trigger,
  });
  return (
    <div className="agent-header-more">
      <button
        ref={trigger}
        className="project-chat__actions"
        type="button"
        aria-label={`Chat actions for ${title} task`}
        aria-controls={loaded ? menuId : undefined}
        aria-haspopup="menu"
        aria-expanded={open}
        disabled={disabled}
        onPointerEnter={preloadMenu}
        onPointerDown={preloadMenu}
        onFocus={preloadMenu}
        onClick={() => {
          setOpeningFocus("first");
          if (open) close();
          else setOpenScope(scope);
        }}
        onKeyDown={(event) => {
          if (coldOpen.onKeyDown(event)) return;
          if (event.key === "ArrowDown" || event.key === "ArrowUp") {
            event.preventDefault();
            setOpeningFocus(event.key === "ArrowUp" ? "last" : "first");
            setOpenScope(scope);
          }
        }}
      >
        <MoreHorizontal size={16} aria-hidden="true" />
      </button>
      {loadFailed ? (
        <ResourceStateNotice
          state="error"
          title="Chat tools could not be loaded."
          detail="Open the menu again to retry."
        />
      ) : null}
      {loaded ? (
        <MenuPart
          part="AgentHeaderMenu"
          close={close}
          focusMoved={coldOpen.focusMoved}
          menuId={menuId}
          onChanges={onChanges}
          open={open}
          openingFocus={openingFocus}
          projectName={projectName}
          trigger={trigger}
        >
          {children}
        </MenuPart>
      ) : null}
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

const HOVER_CARD_WIDTH = 288;

/**
 * The card beside a rested-on rail row, as in Codex: level with the row and
 * starting where the row ends, over the center pane. It never takes the
 * pointer and adds nothing for assistive technology, which reads the row.
 */
export function ProjectChatHoverCard({
  row,
  shownAt,
  title,
  updatedAt,
  project,
}: {
  row: DOMRect;
  /** When the card appeared; its age reads from then. */
  shownAt: number;
  title: string;
  updatedAt?: number;
  project?: string;
}) {
  const age = agentRailHoverAge(updatedAt, shownAt);
  const left = Math.max(
    8,
    Math.min(row.right, window.innerWidth - HOVER_CARD_WIDTH - 8),
  );
  return createPortal(
    <div
      className="agent-rail-hover-card"
      aria-hidden="true"
      style={{ left, top: row.top, width: HOVER_CARD_WIDTH }}
    >
      <div className="agent-rail-hover-card__head">
        <strong>{title}</strong>
        {age ? <span>{age}</span> : null}
      </div>
      {project ? (
        <div className="agent-rail-hover-card__project">
          <Folder size={12} />
          <span>{project}</span>
        </div>
      ) : null}
    </div>,
    document.body,
  );
}
