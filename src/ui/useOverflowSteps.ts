import { useLayoutEffect, useState, type RefObject } from "react";

// Sub-pixel layout may overshoot an edge by under one device pixel.
const SUBPIXEL = 0.5;

/**
 * Fits a row of controls by taking compaction steps, in order, only as far as
 * the row needs. The steps taken are written as tokens of the row's
 * `data-fit` attribute, which its stylesheet keys on (`[data-fit~=step]` hides
 * a label or moves a control into a menu); they run from the lowest-priority
 * control to the highest, so a control only compacts after everything less
 * important already has.
 *
 * Every step only compacts the row, so each fit bisects for the fewest steps
 * after which no child ends past the row's edge, measuring before paint. Text
 * and children that change inside the row refit it at once. A size change of
 * the row or of any child that the last fit did not itself leave refits it on
 * the next frame, which catches style-only changes without refitting for the
 * fit's own steps. While paused (a popover from the row is open) the row keeps
 * its steps, so nothing the person is using moves under them. Returns the
 * steps taken.
 */
export function useOverflowSteps<Step extends string>(
  row: RefObject<HTMLElement | null>,
  steps: readonly Step[],
  paused = false,
): Step[] {
  const [taken, setTaken] = useState(0);
  const key = steps.join(" ");
  useLayoutEffect(() => {
    const element = row.current;
    if (!element || paused) return;
    const names = key.split(" ");
    const take = (count: number) =>
      element.setAttribute("data-fit", names.slice(0, count).join(" "));
    const overflows = (count: number) => {
      take(count);
      const edge = element.getBoundingClientRect().right + SUBPIXEL;
      return [...element.children].some(
        (child) => child.getBoundingClientRect().right > edge,
      );
    };
    const size = (width: number, height: number) => `${width}x${height}`;
    const fitted = new Map<Element, string>();
    let frame = 0;
    const resize =
      typeof ResizeObserver === "undefined"
        ? null
        : new ResizeObserver((entries) => {
            const moved = entries.some(({ target, borderBoxSize: [box] }) => {
              return fitted.get(target) !== size(box.inlineSize, box.blockSize);
            });
            if (!moved) return;
            cancelAnimationFrame(frame);
            frame = requestAnimationFrame(fit);
          });
    const fit = () => {
      let low = 0;
      let high = names.length;
      while (low < high) {
        const middle = (low + high) >> 1;
        if (overflows(middle)) low = middle + 1;
        else high = middle;
      }
      take(low);
      setTaken(low);
      fitted.clear();
      for (const target of [element, ...element.children]) {
        const box = target.getBoundingClientRect();
        fitted.set(target, size(box.width, box.height));
        resize?.observe(target);
      }
    };
    fit();
    // Only content changes refit at once: the steps are attribute writes.
    const content = new MutationObserver(fit);
    content.observe(element, {
      subtree: true,
      childList: true,
      characterData: true,
    });
    return () => {
      if (frame) cancelAnimationFrame(frame);
      resize?.disconnect();
      content.disconnect();
    };
  }, [row, key, paused]);
  return steps.slice(0, taken);
}
