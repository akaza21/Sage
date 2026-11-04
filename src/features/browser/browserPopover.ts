import { useEffect, useRef, type KeyboardEvent, type RefObject } from "react";

/**
 * Closes an open toolbar popover when a pointer goes down outside every one
 * of `within`. A covering popover hides the native page, so outside clicks
 * in the covered page area also land on Sage's DOM.
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
  if (!["ArrowUp", "ArrowDown", "Home", "End"].includes(event.key)) return;
  event.preventDefault();
  const items = Array.from(
    menu?.querySelectorAll<HTMLButtonElement>(
      '[role="menuitem"]:not(:disabled)',
    ) ?? [],
  );
  if (!items.length) return;
  const current = items.indexOf(document.activeElement as HTMLButtonElement);
  const next =
    event.key === "Home"
      ? 0
      : event.key === "End"
        ? items.length - 1
        : event.key === "ArrowDown"
          ? (current + 1) % items.length
          : (current - 1 + items.length) % items.length;
  items[next]?.focus();
}
