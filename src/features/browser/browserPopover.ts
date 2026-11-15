import { useEffect, useRef, type KeyboardEvent, type RefObject } from "react";

import {
  hasNavigationModifier,
  listIndexForKey,
} from "../../ui/listNavigation";

/**
 * A covering popover hides the native page, so outside clicks in the covered
 * page area also land on Sage's DOM.
 */
export function useDismissOnOutsidePointer(
  open: boolean,
  within: readonly RefObject<HTMLElement | null>[],
  close: (open: false) => void,
) {
  const latest = useRef({ within, close });
  useEffect(() => {
    latest.current = { within, close };
  });
  useEffect(() => {
    if (!open) return;
    const onPointerDown = (event: PointerEvent) => {
      const target = event.target;
      if (
        target instanceof Node &&
        latest.current.within.some((ref) => ref.current?.contains(target))
      )
        return;
      latest.current.close(false);
    };
    document.addEventListener("pointerdown", onPointerDown, true);
    return () =>
      document.removeEventListener("pointerdown", onPointerDown, true);
  }, [open]);
}

export function browserMenuKeyDown(
  event: KeyboardEvent<HTMLDivElement>,
  menu: HTMLElement | null,
  dismiss: () => void,
) {
  if (event.key === "Escape") {
    // The menu is the innermost layer: Escape must not reach the pane.
    event.preventDefault();
    event.stopPropagation();
    dismiss();
    return;
  }
  if (
    !["ArrowUp", "ArrowDown", "Home", "End"].includes(event.key) ||
    hasNavigationModifier(event)
  )
    return;
  // Overflowed form controls retain their native navigation keys.
  if ((event.target as Element).closest("select, input, textarea")) return;
  event.preventDefault();
  const items = Array.from(
    menu?.querySelectorAll<HTMLButtonElement>(
      '[role="menuitem"]:not(:disabled)',
    ) ?? [],
  );
  const next = listIndexForKey(
    event.key,
    items.indexOf(document.activeElement as HTMLButtonElement),
    items.map(() => false),
    { orientation: "vertical", wrap: true },
  );
  if (next !== null) items[next].focus();
}
