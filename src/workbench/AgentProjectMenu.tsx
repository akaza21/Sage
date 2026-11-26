import { useId, useRef, useState } from "react";
import { ConfirmDialog } from "../ui/ConfirmDialog";
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
  const [removing, setRemoving] = useState(false);
  const trigger = useRef<HTMLButtonElement>(null);
  const menuId = useId();
  const complete = (action: () => void) => {
    setOpen(false);
    trigger.current?.focus();
    action();
  };
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
        onClick={() => setOpen(!open)}
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
        <button
          className="menu-item"
          type="button"
          role="menuitem"
          onClick={() => complete(onCreateChat)}
        >
          New chat in project
        </button>
        <button
          className="menu-item"
          type="button"
          role="menuitem"
          onClick={() => complete(onOpenProject)}
        >
          Open project
        </button>
        <button
          className="menu-item"
          type="button"
          role="menuitem"
          onClick={() => complete(() => setRemoving(true))}
        >
          Remove project…
        </button>
        {[
          [
            "Rename project",
            "Project renaming is not supported by the native project registry.",
          ],
          [
            "Reveal project",
            "Revealing a project folder is not supported by the native project registry.",
          ],
        ].map(([label, reason]) => (
          <button
            key={label}
            className="menu-item"
            type="button"
            role="menuitem"
            disabled
            title={reason}
            aria-label={`${label}: ${reason}`}
          >
            {label}
          </button>
        ))}
      </MenuPopover>
      {removing ? (
        <ConfirmDialog
          title={`Remove ${name}?`}
          description="This project and all of its chats, archived ones included, will be permanently deleted from Sage. Files in the project folder are not touched. This can't be undone."
          confirmLabel="Remove project"
          onCancel={() => setRemoving(false)}
          onConfirm={() => {
            setRemoving(false);
            void import("../app/chatArchiveActions").then((actions) =>
              actions.cleanUpProjectData("remove_project", projectId),
            );
          }}
        />
      ) : null}
    </div>
  );
}
