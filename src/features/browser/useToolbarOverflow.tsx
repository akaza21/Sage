import { useCallback, useLayoutEffect, useState, type ReactNode } from "react";
import { usePortalHost } from "../../ui/usePortalHost";

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
/** Layers that hang from the toolbar row without taking part in its width. */
const POPOVERS = ".browser-downloads__popover, .browser-surface__menu";
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
  const [slot, setSlot] = useState<HTMLDivElement | null>(null);
  const { host, portal } = usePortalHost(
    slot,
    "browser-surface__page-actions-host",
  );
  const [tier, setTier] = useState(0);
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
    // cannot loop; the actions' content changes arrive as mutations. Changes
    // inside a popover hanging from the row (the Downloads list ticking its
    // progress, the More menu) cannot change the row's width, so they never
    // refit it.
    const resize = new ResizeObserver(fit);
    resize.observe(toolbar);
    const inRow = (record: MutationRecord) => {
      const node =
        record.target instanceof Element
          ? record.target
          : record.target.parentElement;
      return !node?.closest(POPOVERS);
    };
    const content = new MutationObserver((records) => {
      if (records.some(inRow)) fit();
    });
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
        {pageControls ? portal(pageControls) : null}
      </>
    ),
    // Only an open menu holds the actions, so the closed row always fits
    // with them in place.
    menuActions:
      menuOpen && pageControls && tier >= LAST ? (
        <div
          ref={menuSlot}
          role="group"
          aria-label="Page actions"
          className="browser-surface__menu-group browser-surface__menu-page-actions"
        />
      ) : null,
  };
}
