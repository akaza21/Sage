import { MoreHorizontal } from "../ui/leanIcons";
import { useCallback, useId, useRef, useState } from "react";
import { lazyParts } from "../features/chat/lazyParts";
import type * as OptionsPopoverChunk from "./AgentRailOptionsPopover";

const OptionsPopoverPart = lazyParts<typeof OptionsPopoverChunk>(
  () => import("./AgentRailOptionsPopover"),
);

export function AgentRailOptionsMenu() {
  const [open, setOpen] = useState(false);
  const [openingFocus, setOpeningFocus] = useState<"first" | "last">("first");
  const triggerRef = useRef<HTMLButtonElement | null>(null);
  const menuId = useId();

  const closeAndRestoreFocus = useCallback(() => {
    setOpen(false);
    queueMicrotask(() => triggerRef.current?.focus());
    // Reorganizing the sidebar remounts this trigger under another section,
    // so focus follows the new one.
    requestAnimationFrame(() => {
      if (triggerRef.current?.isConnected) return;
      document
        .querySelector<HTMLElement>(
          ".project-sidebar .agent-rail-options__trigger",
        )
        ?.focus();
    });
  }, []);

  return (
    <div className="agent-rail-options">
      <button
        ref={triggerRef}
        className="agent-rail-options__trigger"
        type="button"
        aria-label="Sidebar options"
        aria-haspopup="menu"
        aria-expanded={open}
        aria-controls={open ? menuId : undefined}
        title="Sidebar options"
        onClick={() => {
          setOpeningFocus("first");
          setOpen((value) => !value);
        }}
        onKeyDown={(event) => {
          if (event.key !== "ArrowDown" && event.key !== "ArrowUp") return;
          event.preventDefault();
          setOpeningFocus(event.key === "ArrowUp" ? "last" : "first");
          setOpen(true);
        }}
      >
        <MoreHorizontal size={14} aria-hidden="true" />
      </button>
      {open ? (
        <OptionsPopoverPart
          part="AgentRailOptionsPopover"
          menuId={menuId}
          openingFocus={openingFocus}
          triggerRef={triggerRef}
          onDismiss={() => setOpen(false)}
          onCloseAndRestoreFocus={closeAndRestoreFocus}
        />
      ) : null}
    </div>
  );
}
