import {
  Archive,
  ArrowLeft,
  Folder,
  FolderInput,
  Laptop,
  MoreHorizontal,
  Pencil,
  Pin,
  PinOff,
} from "lucide-react";
import { useEffect, useRef, useState } from "react";

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
}) {
  const localTriggerRef = useRef<HTMLButtonElement | null>(null);
  const menuRef = useRef<HTMLDivElement | null>(null);
  const [moving, setMoving] = useState(false);

  useEffect(() => {
    if (!open) return;
    menuRef.current
      ?.querySelector<HTMLButtonElement>("[role='menuitem']")
      ?.focus();
  }, [moving, open]);

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
        aria-haspopup="menu"
        aria-expanded={open}
        disabled={disabled}
        onClick={() => {
          if (!open) setMoving(false);
          onOpenChange(!open);
        }}
      >
        <MoreHorizontal size={15} aria-hidden="true" />
      </button>
      {open ? (
        <div
          ref={menuRef}
          className="project-chat-actions__menu"
          role="menu"
          aria-label={`Actions for ${chatTitle}`}
          onKeyDown={(event) => {
            if (event.key === "Escape") {
              event.preventDefault();
              event.stopPropagation();
              closeAndRestoreFocus();
              return;
            }
            if (!["ArrowDown", "ArrowUp", "Home", "End"].includes(event.key)) {
              return;
            }
            event.preventDefault();
            event.stopPropagation();
            const items = Array.from(
              event.currentTarget.querySelectorAll<HTMLButtonElement>(
                "button[role='menuitem']:not(:disabled)",
              ),
            );
            if (items.length === 0) return;
            const currentIndex = items.indexOf(
              document.activeElement as HTMLButtonElement,
            );
            const nextIndex =
              event.key === "Home"
                ? 0
                : event.key === "End"
                  ? items.length - 1
                  : event.key === "ArrowDown"
                    ? (currentIndex + 1) % items.length
                    : (currentIndex - 1 + items.length) % items.length;
            items[nextIndex]?.focus();
          }}
        >
          {moving ? (
            <>
              <button
                type="button"
                role="menuitem"
                onClick={() => setMoving(false)}
              >
                <ArrowLeft size={14} aria-hidden="true" />
                <span>Back</span>
              </button>
              <div className="project-chat-actions__label">Move to project</div>
              {destinations.length === 0 ? (
                <p className="project-chat-actions__empty">
                  No other project available.
                </p>
              ) : (
                destinations.map((destination) => (
                  <button
                    key={destination.id}
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
              <button
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
                type="button"
                role="menuitem"
                onClick={() => setMoving(true)}
              >
                <FolderInput size={14} aria-hidden="true" />
                <span>Move to project</span>
              </button>
              <button
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
        </div>
      ) : null}
    </div>
  );
}
