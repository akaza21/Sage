import type { RefObject } from "react";
import type { HarnessDefinition } from "../../domain/harness";
import { MenuPopover } from "../../ui/MenuPopover";
import { HarnessLoginAction } from "./HarnessSignInAction";

export function HarnessSignInMenu({
  menuId,
  triggerRef,
  harnesses,
  onDismiss,
}: {
  menuId: string;
  triggerRef: RefObject<HTMLButtonElement | null>;
  harnesses: readonly HarnessDefinition[];
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
      {harnesses.map((harness) => (
        <HarnessLoginAction key={harness.id} harness={harness} menuItem />
      ))}
    </MenuPopover>
  );
}
