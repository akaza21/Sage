import { useId, useRef, useState } from "react";
import { MoreHorizontal } from "../ui/leanIcons";
import { MenuPopover } from "../ui/MenuPopover";

function Item({ label, onClick }: { label: string; onClick: () => void }) {
  return (
    <button
      className="menu-item"
      type="button"
      role="menuitem"
      onClick={onClick}
    >
      {label}
    </button>
  );
}

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
        ariaDescribedBy={confirming ? `${menuId}-note` : undefined}
        focusKey={confirming}
        onDismiss={() => setOpen(false)}
      >
        {confirming ? (
          <>
            <p className="menu-item" id={`${menuId}-note`}>
              Delete {name} and its chats? Files stay on disk.
            </p>
            <Item label="Cancel" onClick={() => complete(() => {})} />
            <Item
              label="Remove project"
              onClick={() =>
                complete(
                  () =>
                    void import("../app/chatArchiveActions").then((actions) =>
                      actions.cleanUpProjectData("remove_project", projectId),
                    ),
                )
              }
            />
          </>
        ) : (
          <>
            <Item
              label="New chat in project"
              onClick={() => complete(onCreateChat)}
            />
            <Item
              label="Open project"
              onClick={() => complete(onOpenProject)}
            />
            <Item label="Remove project…" onClick={() => setConfirming(true)} />
          </>
        )}
      </MenuPopover>
    </div>
  );
}
