import { useLayoutEffect, useRef, useState } from "react";

export interface ToolbarLayout {
  row: HTMLElement;
  /** Every step must only reduce the space required by the row. */
  steps: number;
  search?: "binary";
  measure: (taken: number) => boolean;
  prepare?: () => void;
  finish?: () => void;
  targets?: () => Element[];
  inRow?: (record: MutationRecord) => boolean;
  focus: {
    items: () => HTMLElement[];
    trigger: () => HTMLElement | null;
  };
}

export function useToolbarOverflow(setup: () => ToolbarLayout | undefined) {
  const [tier, setTier] = useState(-1);
  const remembered = useRef<{ key: string; element: HTMLElement } | null>(null);
  const restore = useRef<(() => void) | null>(null);
  useLayoutEffect(() => {
    const layout = setup();
    if (!layout) {
      remembered.current = null;
      return;
    }
    const { row, steps, measure, prepare, finish, inRow, focus } = layout;
    const key = (element: HTMLElement) =>
      element.closest<HTMLElement>("[data-overflow-control]")?.dataset
        .overflowControl;
    const trackFocus = () => {
      const active = document.activeElement;
      const item = focus.items().find((element) => element === active);
      if (item) remembered.current = { key: key(item)!, element: item };
      else if (active !== remembered.current?.element)
        remembered.current = null;
    };
    const clearPointerFocus = (event: PointerEvent) => {
      if (
        !event.defaultPrevented &&
        !remembered.current?.element.contains(event.target as Node)
      )
        remembered.current = null;
    };
    const restoreFocus = (fitting = false) => {
      const previous = remembered.current;
      if (!previous) return;
      const available = (element: HTMLElement | null) =>
        element &&
        !element.matches(":disabled") &&
        element.getClientRects().length > 0;
      const item = focus
        .items()
        .find((element) => key(element) === previous.key && available(element));
      const target = item ?? focus.trigger();
      if (available(target)) {
        remembered.current = { key: previous.key, element: target! };
        target!.focus({ preventScroll: true });
      } else if (!fitting) remembered.current = null;
    };
    restore.current = restoreFocus;
    document.addEventListener("focusin", trackFocus);
    document.addEventListener("pointerdown", clearPointerFocus);
    const fitted = new Map<Element, string>();
    const size = (target: Element) => {
      const box = target.getBoundingClientRect();
      return box.width + "x" + box.height;
    };
    let frame = 0;
    const resize =
      typeof ResizeObserver === "undefined"
        ? null
        : new ResizeObserver((entries) => {
            if (
              !entries.some(({ target }) => fitted.get(target) !== size(target))
            )
              return;
            if (frame) cancelAnimationFrame(frame);
            frame = requestAnimationFrame(fit);
          });
    const fit = () => {
      if (frame) cancelAnimationFrame(frame);
      frame = 0;
      if (document.activeElement !== document.body) trackFocus();
      prepare?.();
      let next = 0;
      if (measure(0) && steps > 0) {
        next = 1;
        let last = steps;
        while (next < last) {
          const probe = layout.search === "binary" ? (next + last) >> 1 : next;
          if (measure(probe)) next = probe + 1;
          else last = probe;
        }
        measure(next);
      }
      finish?.();
      restoreFocus(true);
      content.takeRecords();
      const targets = layout.targets?.() ?? [row, ...row.children];
      for (const target of fitted.keys())
        if (!targets.includes(target)) resize?.unobserve(target);
      fitted.clear();
      for (const target of targets) {
        fitted.set(target, size(target));
        resize?.observe(target);
      }
      setTier(next);
    };
    const content = new MutationObserver((records) => {
      if (!inRow || records.some(inRow)) fit();
    });
    content.observe(row, {
      subtree: true,
      childList: true,
      characterData: true,
    });
    fit();
    return () => {
      if (frame) cancelAnimationFrame(frame);
      resize?.disconnect();
      content.disconnect();
      document.removeEventListener("focusin", trackFocus);
      document.removeEventListener("pointerdown", clearPointerFocus);
      restore.current = null;
    };
  }, [setup]);
  // React may replace the focused presentation only after the fit publishes
  // its tier. Restore again before paint, once the replacement has mounted.
  useLayoutEffect(() => {
    restore.current?.();
  });
  return tier;
}
