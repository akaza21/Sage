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
    const name = label(previous);
    // Focus the same-named control, or keep waiting while the new rail is
    // still loading. Give up once focus has gone anywhere else.
    const settle = () => {
      if ((document.activeElement ?? document.body) !== document.body)
        return true;
      for (const control of document.querySelectorAll<HTMLElement>(
        ".project-sidebar button",
      ))
        if (label(control) === name) {
          control.focus();
          return true;
        }
      return false;
    };
    if (settle()) return;
    const observer = new MutationObserver(() => {
      if (settle()) observer.disconnect();
    });
    observer.observe(document.body, { childList: true, subtree: true });
    return () => observer.disconnect();
  }, [collapsed]);
}
