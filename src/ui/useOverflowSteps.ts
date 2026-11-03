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
 * Each fit starts from the full row and adds one step at a time until no
 * child ends past the row's edge, writing and measuring every step
 * before paint. Text and children that change inside the row refit it at
 * once; a resize of the row or of any child refits it on the next frame,
 * which also lets the last steps reshape the row itself without a loop. While paused (a popover from
 * the row is open) the row keeps its steps, so nothing the person is using
 * moves under them. Returns the steps taken.
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
    const overflows = () => {
      const edge = element.getBoundingClientRect().right + SUBPIXEL;
      return [...element.children].some(
        (child) => child.getBoundingClientRect().right > edge,
      );
    };
    let frame = 0;
    // A child that changes size without a content change (a class or
    // attribute flip, a late stylesheet) refits too. Settled steps leave the
    // children at the size they were observed at, so this never loops.
    const resize =
      typeof ResizeObserver === "undefined"
        ? null
        : new ResizeObserver(() => {
            cancelAnimationFrame(frame);
            frame = requestAnimationFrame(fit);
          });
    const fit = () => {
      let count = 0;
      element.setAttribute("data-fit", "");
      while (count < names.length && overflows())
        element.setAttribute("data-fit", names.slice(0, ++count).join(" "));
      setTaken(count);
      for (const child of [element, ...element.children])
        resize?.observe(child);
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
