import type { ReactNode, RefObject } from "react";
import { MenuPopover } from "../../ui/MenuPopover";

export function HarnessSignInMenu({
  menuId,
  triggerRef,
  children,
  onDismiss,
}: {
  menuId: string;
  triggerRef: RefObject<HTMLButtonElement | null>;
  children: ReactNode;
  onDismiss: () => void;
}) {
  return (
    <MenuPopover
      id={menuId}
      open
      triggerRef={triggerRef}
      ariaLabel="Harness sign-in"
      placement="above-start"
      initialFocus="first"
      className="harness-account__menu"
      onDismiss={onDismiss}
    >
      {children}
    </MenuPopover>
  );
}
