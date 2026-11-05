import { useLayoutEffect, useState } from "react";

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
}

/**
 * One bounded fitter for toolbars. Adapters supply their presentation and
 * overflow measurement; this owns the step walk and observation lifecycle.
 * Size changes caused by the fit itself do not schedule another fit. Content
 * changes fit before paint, including controls relocated into an open More.
 * Unmounting (or returning no layout) disconnects all work.
 */
export function useToolbarOverflow(setup: () => ToolbarLayout | undefined) {
  const [tier, setTier] = useState(0);
  useLayoutEffect(() => {
    const layout = setup();
    if (!layout) return;
    const { row, steps, measure, prepare, finish, inRow } = layout;
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
    };
  }, [setup]);
  return tier;
}
