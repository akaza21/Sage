import { useCallback, useLayoutEffect, useState, type ReactNode } from "react";
import { createPortal } from "react-dom";

/**
 * The toolbar's compaction steps, in order; each is an attribute on the
 * toolbar that browser.css keys on. A tier is how many have been taken.
 */
const STEPS = [
  "data-compact-annotate",
  "data-compact-chat",
  "data-compact-presence",
  "data-overflow-downloads",
  "data-overflow-actions",
] as const;
const LAST = STEPS.length;
// Sub-pixel layout may overshoot an edge by under one device pixel.
const SUBPIXEL = 0.5;

/**
 * Compacts secondary toolbar actions, and moves them into the More menu,
 * only as far as the toolbar row needs, as in Chrome and Edge. The address
 * keeps its token minimum, so each fit lays the row out in full and takes
 * the next step until it fits; every step is written to the toolbar and
 * measured before paint, and a fit always starts again from the full row,
 * so actions that appear later (an Agent attaching) never count twice.
 *
 * The page actions render once into a host that moves between the toolbar
 * and the menu, so they keep their state (the chosen chat, a pending attach)
 * and behave identically wherever they are shown.
 */
export function useToolbarOverflow(menuOpen: boolean, pageControls: ReactNode) {
  const [host] = useState(() => {
    const element = document.createElement("div");
    element.className = "browser-surface__page-actions-host";
    return element;
  });
  const [slot, setSlot] = useState<HTMLDivElement | null>(null);
  const [tier, setTier] = useState(0);
  useLayoutEffect(() => slot?.append(host), [slot, host]);
  useLayoutEffect(() => {
    const group = slot?.parentElement;
    const toolbar = group?.closest<HTMLElement>(".browser-surface__toolbar");
    if (!group || !toolbar) return;
    return () => {
      for (const step of STEPS) toolbar.removeAttribute(step);
    };
  }, [slot]);
  useLayoutEffect(() => {
    const group = slot?.parentElement;
    const toolbar = group?.closest<HTMLElement>(".browser-surface__toolbar");
    // While the menu is open it holds the moved actions, so the row would
    // measure without them: keep the tier and refit once it closes.
    if (!group || !toolbar || menuOpen || typeof ResizeObserver === "undefined")
      return;
    const apply = (taken: number) =>
      STEPS.forEach((step, index) =>
        toolbar.toggleAttribute(step, index < taken),
      );
    // The right group ends the row, so the row overflows when the group ends
    // past the toolbar's content edge.
    const overflows = () =>
      group.getBoundingClientRect().right >
      toolbar.getBoundingClientRect().right -
        parseFloat(getComputedStyle(toolbar).paddingRight) +
        SUBPIXEL;
    const fit = () => {
      let next = 0;
      apply(next);
      while (next < LAST && overflows()) apply(++next);
      setTier(next);
    };
    fit();
    // The toolbar's own size never depends on the steps, so observing it
    // cannot loop; the actions' content changes arrive as mutations.
    const resize = new ResizeObserver(fit);
    resize.observe(toolbar);
    const content = new MutationObserver(fit);
    content.observe(group, {
      subtree: true,
      childList: true,
      characterData: true,
    });
    return () => {
      resize.disconnect();
      content.disconnect();
    };
  }, [slot, menuOpen]);
  const menuSlot = useCallback(
    (node: HTMLDivElement | null) => {
      if (!node) return;
      node.append(host);
      return () => slot?.append(host);
    },
    [host, slot],
  );
  return {
    toolbarActions: (
      <>
        <div ref={setSlot} className="browser-surface__page-actions-slot" />
        {pageControls ? createPortal(pageControls, host) : null}
      </>
    ),
    menuActions:
      pageControls && tier >= LAST ? (
        <div
          ref={menuSlot}
          role="group"
          aria-label="Page actions"
          className="browser-surface__menu-group browser-surface__menu-page-actions"
        />
      ) : null,
  };
}
