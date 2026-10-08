import {
  Archive,
  ArrowLeft,
  ArrowRightLeft,
  Folder,
  FolderInput,
  Laptop,
  MoreHorizontal,
  Pencil,
  Pin,
  PinOff,
} from "../ui/leanIcons";
import { useId, useRef, useState } from "react";

import { MenuPopover } from "../ui/MenuPopover";
import { ChatHandoffControl } from "../features/chat/ChatHandoffControl";
import type { AgentSessionChrome } from "../app/agentChatOwner";
import type { AgentChatMoveDestination } from "./agentChatRail";

export function ProjectChatActionMenu({
  chatTitle,
  pinned,
  open,
  disabled,
  archiveDisabled,
  destinations,
  onOpenChange,
  onPin,
  onRename,
  onMove,
  onArchive,
  triggerRef,
  sessionChrome,
}: {
  chatTitle: string;
  pinned: boolean;
  open: boolean;
  disabled: boolean;
  archiveDisabled: boolean;
  destinations: readonly AgentChatMoveDestination[];
  onOpenChange: (open: boolean) => void;
  onPin: () => void;
  onRename: () => void;
  onMove: (projectId: string) => void;
  onArchive: () => void;
  triggerRef: (node: HTMLButtonElement | null) => void;
  sessionChrome?: AgentSessionChrome | null;
}) {
  const localTriggerRef = useRef<HTMLButtonElement | null>(null);
  const [openingFocus, setOpeningFocus] = useState<"first" | "last">("first");
  const [moving, setMoving] = useState(false);
  const menuId = useId();

  const closeAndRestoreFocus = () => {
    setMoving(false);
    localTriggerRef.current?.focus();
    onOpenChange(false);
  };

  const completeAction = (action: () => void) => {
    closeAndRestoreFocus();
    action();
  };

  return (
    <div className="project-chat-actions">
      <button
        ref={(node) => {
          localTriggerRef.current = node;
          triggerRef(node);
        }}
        className="project-chat__actions"
        type="button"
        aria-label={`Chat actions for ${chatTitle}`}
        aria-controls={menuId}
        aria-haspopup="menu"
        aria-expanded={open}
        disabled={disabled}
        onClick={() => {
          setOpeningFocus("first");
          setMoving(false);
          onOpenChange(!open);
        }}
        onKeyDown={(event) => {
          if (event.key !== "ArrowDown" && event.key !== "ArrowUp") return;
          event.preventDefault();
          setOpeningFocus(event.key === "ArrowUp" ? "last" : "first");
          setMoving(false);
          onOpenChange(true);
        }}
      >
        <MoreHorizontal size={16} aria-hidden="true" />
      </button>
      <MenuPopover
        id={menuId}
        open={open}
        ariaLabel={`Actions for ${chatTitle}`}
        className="project-chat-actions__menu"
        focusKey={moving}
        initialFocus={openingFocus}
        triggerRef={localTriggerRef}
        keepMounted={Boolean(sessionChrome)}
        onDismiss={() => {
          setMoving(false);
          onOpenChange(false);
        }}
      >
        {moving ? (
          <>
            <button
              className="menu-item"
              type="button"
              role="menuitem"
              onClick={() => setMoving(false)}
            >
              <ArrowLeft size={14} aria-hidden="true" />
              <span>Back</span>
            </button>
            <div className="menu-group-label">Move to project</div>
            {destinations.length === 0 ? (
              <p className="menu-empty">No other project available.</p>
            ) : (
              destinations.map((destination) => (
                <button
                  key={destination.id}
                  className="menu-item"
                  type="button"
                  role="menuitem"
                  aria-label={`Move chat to ${destination.name}`}
                  onClick={() => completeAction(() => onMove(destination.id))}
                >
                  {destination.kind === "local" ? (
                    <Laptop size={14} aria-hidden="true" />
                  ) : (
                    <Folder size={14} aria-hidden="true" />
                  )}
                  <span>{destination.name}</span>
                </button>
              ))
            )}
          </>
        ) : (
          <>
            {sessionChrome ? (
              <>
                <ChatHandoffControl
                  acpPage={sessionChrome.acpPage}
                  messages={sessionChrome.messages}
                  busy={sessionChrome.busy}
                  onCommitted={() =>
                    sessionChrome.actions.current.focusComposer()
                  }
                  menuItem
                  returnFocusRef={localTriggerRef}
                  onOpen={() => onOpenChange(false)}
                />
                {sessionChrome.canCloseSession ? (
                  <button
                    className="menu-item"
                    type="button"
                    role="menuitem"
                    disabled={sessionChrome.busy}
                    onClick={() =>
                      completeAction(() =>
                        sessionChrome.actions.current.closeSession(),
                      )
                    }
                  >
                    <ArrowRightLeft size={14} aria-hidden="true" />
                    <span>Close session</span>
                  </button>
                ) : null}
                {sessionChrome.canEndSession ? (
                  <button
                    className="menu-item"
                    type="button"
                    role="menuitem"
                    disabled={sessionChrome.busy}
                    onClick={() =>
                      completeAction(() =>
                        sessionChrome.actions.current.endSession(),
                      )
                    }
                  >
                    <ArrowRightLeft size={14} aria-hidden="true" />
                    <span>End session</span>
                  </button>
                ) : null}
              </>
            ) : null}
            <button
              className="menu-item"
              type="button"
              role="menuitem"
              onClick={() => completeAction(onPin)}
            >
              {pinned ? (
                <PinOff size={14} aria-hidden="true" />
              ) : (
                <Pin size={14} aria-hidden="true" />
              )}
              <span>{pinned ? "Unpin chat" : "Pin chat"}</span>
            </button>
            <button
              className="menu-item"
              type="button"
              role="menuitem"
              onClick={() => setMoving(true)}
            >
              <FolderInput size={14} aria-hidden="true" />
              <span>Move to project</span>
            </button>
            <button
              className="menu-item"
              type="button"
              role="menuitem"
              onClick={() => {
                onOpenChange(false);
                onRename();
              }}
            >
              <Pencil size={14} aria-hidden="true" />
              <span>Rename chat</span>
            </button>
            <button
              className="menu-item"
              type="button"
              role="menuitem"
              disabled={archiveDisabled}
              onClick={() => completeAction(onArchive)}
            >
              <Archive size={14} aria-hidden="true" />
              <span>Archive chat</span>
            </button>
          </>
        )}
      </MenuPopover>
    </div>
  );
}
