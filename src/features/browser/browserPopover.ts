import { useEffect, useRef, type RefObject } from "react";

/**
 * Closes an open toolbar popover when a pointer goes down outside every one
 * of `within`. The page hides while a popover is open, so a click over the
 * page area lands on Sage's DOM too.
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
