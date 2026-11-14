import { useId, useRef, type ReactNode } from "react";

import { ChevronRight } from "./leanIcons";
import { MenuPopover } from "./MenuPopover";
import type { SubmenuGrace } from "./submenuGrace";

export function MenuSubmenu({
  label,
  leading,
  className,
  menuClassName,
  open,
  onOpenChange,
  grace,
  children,
}: {
  label: string;
  leading?: ReactNode;
  className: string;
  menuClassName: string;
  open: boolean;
  onOpenChange: (open: boolean) => void;
  grace: SubmenuGrace;
  children: ReactNode;
}) {
  const triggerRef = useRef<HTMLButtonElement>(null);
  const pointerArrived = useRef(false);
  const submenuId = useId();
  const dismiss = () => {
    grace.reset();
    onOpenChange(false);
  };
  const close = () => {
    pointerArrived.current = false;
    dismiss();
    triggerRef.current?.focus();
  };
  return (
    <>
      <button
        ref={triggerRef}
        className={className}
        type="button"
        role="menuitem"
        tabIndex={-1}
        aria-haspopup="menu"
        aria-expanded={open}
        aria-controls={open ? submenuId : undefined}
        onPointerEnter={() => {
          pointerArrived.current = true;
        }}
        onPointerMove={(event) => {
          if (pointerArrived.current && !open && !grace.holds(event))
            onOpenChange(true);
        }}
        onPointerLeave={(event) => {
          if (open) grace.leave(event, document.getElementById(submenuId));
        }}
        onClick={() => onOpenChange(true)}
        onKeyDown={(event) => {
          if (event.key !== "ArrowRight") return;
          event.preventDefault();
          event.stopPropagation();
          onOpenChange(true);
        }}
      >
        {leading}
        <span>{label}</span>
        <ChevronRight
          className="menu-submenu__chevron"
          size={14}
          aria-hidden="true"
        />
      </button>
      <MenuPopover
        id={submenuId}
        open={open}
        ariaLabel={label}
        className={menuClassName}
        initialFocus="selected"
        portalTarget="closest-popover"
        triggerRef={triggerRef}
        onDismiss={(reason) => {
          if (reason === "escape") close();
          else dismiss();
        }}
      >
        <div
          className="menu-submenu__items"
          onKeyDown={(event) => {
            if (event.key !== "ArrowLeft") return;
            event.preventDefault();
            event.stopPropagation();
            close();
          }}
        >
          {children}
        </div>
      </MenuPopover>
    </>
  );
}
