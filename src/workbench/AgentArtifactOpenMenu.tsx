import type { RefObject } from "react";
import { Copy, SquareArrowOutUpRight } from "../ui/leanIcons";
import { MenuPopover } from "../ui/MenuPopover";
export function AgentArtifactOpenMenu({
  id,
  trigger,
  dismiss,
  onOpenInEditor,
  copyPath,
}: {
  id: string;
  trigger: RefObject<HTMLButtonElement | null>;
  dismiss: () => void;
  onOpenInEditor: () => void;
  copyPath: () => Promise<void>;
}) {
  return (
    <MenuPopover
      id={id}
      open
      ariaLabel="Open file"
      className="menu-popover--editor"
      triggerRef={trigger}
      onDismiss={() => dismiss()}
    >
      <button
        className="menu-item"
        type="button"
        role="menuitem"
        onClick={() => {
          dismiss();
          onOpenInEditor();
          trigger.current?.focus();
        }}
      >
        <SquareArrowOutUpRight size={16} aria-hidden="true" />
        Open in Editor
      </button>
      <button
        className="menu-item"
        type="button"
        role="menuitem"
        onClick={() => {
          dismiss();
          void copyPath();
          trigger.current?.focus();
        }}
      >
        <Copy size={16} aria-hidden="true" />
        Copy path
      </button>
    </MenuPopover>
  );
}
