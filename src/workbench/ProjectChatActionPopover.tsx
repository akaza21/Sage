import {
  Archive,
  ArrowLeft,
  ArrowRightLeft,
  Folder,
  FolderInput,
  Laptop,
  Pencil,
  Pin,
  PinOff,
} from "../ui/leanIcons";
import { useContext, useState, type RefObject } from "react";
import { MenuPopover } from "../ui/MenuPopover";
import { AgentHeaderMenuContext } from "./AgentToolsProvider";
import { ChatHandoffControl } from "../features/chat/ChatHandoffControl";
import type { ProjectChatMenuProps } from "./ProjectChatActionMenu";
export function ProjectChatActionPopover({
  chatTitle,
  pinned,
  open,
  archiveDisabled,
  destinations,
  onOpenChange,
  onPin,
  onRename,
  onMove,
  onArchive,
  onMarkUnread,
  sessionChrome,
  triggerRole,
  menuId,
  openingFocus,
  localTriggerRef,
}: ProjectChatMenuProps & {
  menuId: string;
  openingFocus: "first" | "last";
  localTriggerRef: RefObject<HTMLButtonElement | null>;
}) {
  const [moving, setMoving] = useState(false);
  const headerMenu = useContext(AgentHeaderMenuContext);
  const closeAndRestoreFocus = () => {
    setMoving(false);
    localTriggerRef.current?.focus();
    onOpenChange(false);
  };

  const completeAction = (action: () => void) => {
    closeAndRestoreFocus();
    headerMenu?.close();
    headerMenu?.returnFocus();
    action();
  };

  return (
    <MenuPopover
      id={menuId}
      open={open}
      ariaLabel={`Actions for ${chatTitle}`}
      className="project-chat-actions__menu"
      focusKey={moving}
      initialFocus={openingFocus}
      triggerRef={localTriggerRef}
      portalTarget={triggerRole ? "closest-popover" : "document"}
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
                onReturnFocus={headerMenu?.returnFocus}
                onOpen={() => {
                  onOpenChange(false);
                  headerMenu?.close();
                }}
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
          {onMarkUnread ? (
            <button
              className="menu-item"
              type="button"
              role="menuitem"
              onClick={() => completeAction(onMarkUnread)}
            >
              Mark unread
            </button>
          ) : null}
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
  );
}
