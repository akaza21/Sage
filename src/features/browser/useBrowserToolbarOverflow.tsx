import { useCallback, useLayoutEffect, useRef, type ReactNode } from "react";
import { useToolbarOverflow } from "../../ui/useToolbarOverflow";
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
const POPOVERS = ".browser-downloads__popover";
// Sub-pixel layout may overshoot an edge by under one device pixel.
const SUBPIXEL = 0.5;

/** Reparenting must preserve keyboard interaction with a moved control. */
function moveActions(
  host: HTMLElement,
  target: HTMLElement,
  focused = host.contains(document.activeElement)
    ? (document.activeElement as HTMLElement)
    : null,
) {
  if (host.parentElement !== target) target.append(host);
  if (!focused || !host.contains(focused)) return null;
  focused.focus({ preventScroll: true });
  // A hidden row can retain activeElement until the next focus update.
  return document.activeElement === focused &&
    focused.getClientRects().length > 0
    ? null
    : focused;
}

/**
 * Compacts secondary toolbar actions, and moves them into the More menu,
 * only as far as the toolbar row needs, as in Chrome and Edge. The address
 * keeps its token minimum, so each fit lays the row out in full and takes
 * the next step until it fits; every step is written to the toolbar and
 * measured before paint, and a fit always starts again from the full row,
 * so actions that appear later (an Agent attaching) never count twice.
 *
 * The page-action state owner stays in one portal host. Its presentation
 * and reads sleep at the final tier until More opens; moving the host
 * preserves the chosen chat and pending operations.
 */
export function useBrowserToolbarOverflow(
  menuOpen: boolean,
  pageControls?: (visible: boolean) => ReactNode,
) {
  const slot = useRef<HTMLDivElement | null>(null);
  const { host, portal } = usePortalHost(
    slot,
    "browser-surface__page-actions-host",
  );
  const actionWidths = useRef<number[]>([]);
  const pendingFocus = useRef<HTMLElement | null>(null);
  const tier = useToolbarOverflow(
    useCallback(() => {
      const rowSlot = slot.current;
      const group = rowSlot?.parentElement;
      const toolbar = group?.closest<HTMLElement>(".browser-surface__toolbar");
      if (
        !rowSlot ||
        !group ||
        !toolbar ||
        typeof ResizeObserver === "undefined"
      )
        return;
      const apply = (taken: number) =>
        STEPS.forEach((step, index) =>
          toolbar.toggleAttribute(step, index < taken),
        );
      // One width per compaction tier lets the row refit while the controls
      // sleep. No hidden control DOM or native reads are needed for measurement.
      const widths = actionWidths.current;
      let parent: HTMLElement | null;
      let focused: HTMLElement | null;
      let live = false;
      let edge = 0;
      return {
        row: group,
        steps: LAST,
        targets: () => [toolbar],
        prepare: () => {
          edge =
            toolbar.getBoundingClientRect().right -
            parseFloat(getComputedStyle(toolbar).paddingRight) +
            SUBPIXEL;
          parent = host.parentElement;
          focused = host.contains(document.activeElement)
            ? (document.activeElement as HTMLElement)
            : pendingFocus.current;
          live =
            (menuOpen || !toolbar.hasAttribute(STEPS[LAST - 1])) &&
            host.hasChildNodes();
          moveActions(host, rowSlot);
        },
        measure: (taken: number) => {
          apply(taken);
          rowSlot.style.width = live ? "" : `${widths[taken] ?? 0}px`;
          if (live && taken < LAST)
            widths[taken] = rowSlot.getBoundingClientRect().width;
          return group.getBoundingClientRect().right > edge;
        },
        finish: () => {
          if (parent) pendingFocus.current = moveActions(host, parent, focused);
        },
        inRow: (record: MutationRecord) => {
          const node =
            record.target instanceof Element
              ? record.target
              : record.target.parentElement;
          return !node?.closest(POPOVERS);
        },
      };
    }, [host, menuOpen]),
  );
  const visible = menuOpen || tier < LAST;
  useLayoutEffect(() => {
    if (!visible) pendingFocus.current = null;
  }, [visible]);
  const menuSlot = useCallback(
    (node: HTMLDivElement | null) => {
      if (!node) return;
      const relocate = (target: HTMLElement) => {
        pendingFocus.current = moveActions(
          host,
          target,
          pendingFocus.current ?? undefined,
        );
      };
      relocate(node);
      return () => {
        if (slot.current) relocate(slot.current);
      };
    },
    [host],
  );
  return {
    toolbarActions: (
      <>
        <div ref={slot} className="browser-surface__page-actions-slot" />
        {pageControls && portal(pageControls(visible))}
      </>
    ),
    // The live controls move as one host; sleeping controls use cached widths.
    menuActions: menuOpen && pageControls && tier >= LAST && (
      <div
        ref={menuSlot}
        role="group"
        aria-label="Page actions"
        className="browser-surface__menu-group browser-surface__menu-page-actions"
      />
    ),
  };
}
