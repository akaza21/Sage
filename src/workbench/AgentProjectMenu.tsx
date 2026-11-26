import { useId, useRef, useState } from "react";
import { MoreHorizontal } from "../ui/leanIcons";
import { MenuPopover } from "../ui/MenuPopover";

export function AgentProjectMenu({
  projectId,
  name,
  onCreateChat,
  onOpenProject,
}: {
  projectId: string;
  name: string;
  onCreateChat: () => void;
  onOpenProject: () => void;
}) {
  const [open, setOpen] = useState(false);
  // Removal is permanent, so the menu asks again in place before it runs.
  const [confirming, setConfirming] = useState(false);
  const trigger = useRef<HTMLButtonElement>(null);
  const menuId = useId();
  const complete = (action: () => void) => {
    setOpen(false);
    trigger.current?.focus();
    action();
  };
  const item = (label: string, action: () => void) => (
    <button
      className="menu-item"
      type="button"
      role="menuitem"
      onClick={action}
    >
      {label}
    </button>
  );
  return (
    <div className="project-chat-actions">
      <button
        ref={trigger}
        type="button"
        className="project-chat__actions"
        aria-label={`Project actions for ${name}`}
        aria-haspopup="menu"
        aria-expanded={open}
        aria-controls={menuId}
        onClick={() => {
          setConfirming(false);
          setOpen(!open);
        }}
      >
        <MoreHorizontal size={16} aria-hidden="true" />
      </button>
      <MenuPopover
        id={menuId}
        open={open}
        triggerRef={trigger}
        ariaLabel={`Project actions for ${name}`}
        onDismiss={() => setOpen(false)}
      >
        {confirming ? (
          <>
            <p className="menu-item" role="note">
              Delete {name} and all its chats from Sage? Its folder stays on
              disk. This can't be undone.
            </p>
            {item("Cancel", () => complete(() => {}))}
            {item("Remove project", () =>
              complete(
                () =>
                  void import("../app/chatArchiveActions").then((actions) =>
                    actions.cleanUpProjectData("remove_project", projectId),
                  ),
              ),
            )}
          </>
        ) : (
          <>
            {item("New chat in project", () => complete(onCreateChat))}
            {item("Open project", () => complete(onOpenProject))}
            {item("Remove project…", () => setConfirming(true))}
          </>
        )}
      </MenuPopover>
    </div>
  );
}
