import { useEffect, useLayoutEffect, useRef, type RefObject } from "react";

const POPOVER_EDGE_CLEARANCE = 8;

/** Bound the entire popup, including its heading, to the visible browser pane. */
export function useBoundedBrowserPopover(
  open: boolean,
  popup: RefObject<HTMLElement | null>,
) {
  useLayoutEffect(() => {
    const element = popup.current;
    const pane = element?.closest(".browser-surface");
    if (!open || !element) return;
    const measure = () => {
      const bounds = pane?.getBoundingClientRect();
      const rect = element.getBoundingClientRect();
      const bottom = Math.min(
        window.innerHeight,
        bounds?.bottom ?? window.innerHeight,
      );
      const left = Math.max(0, bounds?.left ?? 0);
      element.style.setProperty(
        "--browser-popup-max-height",
        `${Math.max(0, bottom - rect.top - POPOVER_EDGE_CLEARANCE)}px`,
      );
      element.style.setProperty(
        "--browser-popup-max-width",
        `${Math.max(0, rect.right - left - POPOVER_EDGE_CLEARANCE)}px`,
      );
    };
    measure();
    const observer = pane ? new ResizeObserver(measure) : null;
    if (pane) observer?.observe(pane);
    window.addEventListener("resize", measure);
    return () => {
      observer?.disconnect();
      window.removeEventListener("resize", measure);
    };
  }, [open, popup]);
}

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
