import { useCallback, useLayoutEffect, useState, type ReactNode } from "react";
import { createPortal } from "react-dom";

/** Tier 1 moves Downloads into the More menu; tier 2 adds the page actions. */
const OVERFLOW_TIERS = 2;
// Sub-pixel layout may overshoot an edge by under one device pixel.
const SUBPIXEL = 0.5;

/**
 * Moves secondary toolbar actions into the More menu when the toolbar row
 * would overflow, as in Chrome and Edge. The address keeps its token minimum,
 * so the row sheds the next tier until it fits; each tier is written to the
 * toolbar and measured before paint.
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
      delete toolbar.dataset.overflow;
    };
  }, [slot]);
  useLayoutEffect(() => {
    const group = slot?.parentElement;
    const toolbar = group?.closest<HTMLElement>(".browser-surface__toolbar");
    // While the menu is open it holds the moved actions, so the row would
    // measure without them: keep the tier and refit once it closes.
    if (!group || !toolbar || menuOpen || typeof ResizeObserver === "undefined")
      return;
    const apply = (next: number) => {
      if (next) toolbar.dataset.overflow = String(next);
      else delete toolbar.dataset.overflow;
    };
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
      while (next < OVERFLOW_TIERS && overflows()) apply(++next);
      setTier(next);
    };
    fit();
    const resize = new ResizeObserver(fit);
    resize.observe(toolbar);
    resize.observe(group);
    return () => resize.disconnect();
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
      pageControls && tier >= OVERFLOW_TIERS ? (
        <div
          ref={menuSlot}
          role="group"
          aria-label="Page actions"
          className="browser-surface__menu-group browser-surface__menu-page-actions"
        />
      ) : null,
  };
}
