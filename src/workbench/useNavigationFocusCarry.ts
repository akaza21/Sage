import { useEffect, useLayoutEffect, useRef } from "react";

const label = (element: Element) =>
  element.getAttribute("aria-label") ?? element.textContent?.trim() ?? "";

/**
 * Folding or unfolding the Agent rail swaps the icon rail for the full rail,
 * which unmounts the focused control. When focus was in the rail, carry it to
 * the control with the same name in the new rail instead of dropping it to
 * the document body.
 */
export function useNavigationFocusCarry(collapsed: boolean) {
  const focused = useRef<Element | null>(null);
  useEffect(() => {
    const enter = (event: FocusEvent) => {
      const target = event.target as Element;
      focused.current = target.closest?.(".project-sidebar") ? target : null;
    };
    // Forget a deliberate blur, but keep a control the swap removed: some
    // engines fire focusout during removal, so judge once it has settled.
    const leave = (event: FocusEvent) => {
      const target = event.target as Element;
      queueMicrotask(() => {
        if (focused.current === target && target.isConnected)
          focused.current = null;
      });
    };
    document.addEventListener("focusin", enter);
    document.addEventListener("focusout", leave);
    return () => {
      document.removeEventListener("focusin", enter);
      document.removeEventListener("focusout", leave);
    };
  }, []);
  useLayoutEffect(() => {
    const previous = focused.current;
    if (!previous || previous.isConnected) return;
    focused.current = null;
    if ((document.activeElement ?? document.body) !== document.body) return;
    const name = label(previous);
    for (const control of document.querySelectorAll<HTMLElement>(
      ".project-sidebar button",
    ))
      if (label(control) === name) return control.focus();
  }, [collapsed]);
}
