import { useCallback, useEffect, useLayoutEffect, useRef } from "react";

const label = (element: Element) =>
  element.getAttribute("aria-label") ?? element.textContent?.trim() ?? "";

/**
 * Folding or unfolding the Agent rail swaps the icon rail for the full rail,
 * which unmounts the focused control. When focus was in the rail, carry it to
 * the control with the same name in the new rail (or its first control)
 * instead of dropping it to the document body. The folded rail loads lazily,
 * so each rail reports its own mount through the returned callback ref and
 * focus is restored once, at that moment. Any other focus in the meantime
 * cancels the restore.
 */
export function useNavigationFocusCarry(collapsed: boolean) {
  const focused = useRef<Element | null>(null);
  const pending = useRef<string | null>(null);
  const rail = useRef<HTMLElement | null>(null);
  useEffect(() => {
    const enter = (event: FocusEvent) => {
      pending.current = null;
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
  const restore = useCallback((node: HTMLElement) => {
    const name = pending.current;
    pending.current = null;
    if (name === null) return;
    if ((document.activeElement ?? document.body) !== document.body) return;
    const controls = [...node.querySelectorAll<HTMLElement>("button")];
    (
      controls.find((control) => label(control) === name) ?? controls[0]
    )?.focus();
  }, []);
  useLayoutEffect(() => {
    const previous = focused.current;
    pending.current = null;
    if (!previous || previous.isConnected) return;
    focused.current = null;
    pending.current = label(previous);
    // A rail that mounted in this commit is ready now; a lazy one restores
    // from its ref when it arrives.
    if (rail.current) restore(rail.current);
  }, [collapsed, restore]);
  return useCallback(
    (node: HTMLElement | null) => {
      rail.current = node;
      if (node) restore(node);
    },
    [restore],
  );
}
