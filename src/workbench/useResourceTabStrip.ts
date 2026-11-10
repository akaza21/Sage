import { useLayoutEffect, type KeyboardEvent, type RefObject } from "react";

import {
  handleTabListKeyDown,
  tabListTabs,
  type TabListActivation,
} from "../ui/tabListKeys";

/** A tab's row: the `.tab` wrapper that also holds its close button. */
function tabRow(tab: HTMLElement): HTMLElement {
  return tab.closest<HTMLElement>(".tab") ?? tab;
}

/**
 * Scrolls only the strip, never its ancestors, so the row is fully in view.
 * `scrollIntoView` would also scroll any scrollable pane around the strip.
 */
function revealInStrip(strip: HTMLElement, row: HTMLElement) {
  const bounds = strip.getBoundingClientRect();
  const rect = row.getBoundingClientRect();
  if (rect.left < bounds.left) strip.scrollLeft -= bounds.left - rect.left;
  else if (rect.right > bounds.right)
    strip.scrollLeft += rect.right - bounds.right;
}

/**
 * Keeps one Tab stop on the selected tab, or the first tab when none is
 * selected, and returns the tab the reader is on: the focused tab while focus
 * is in the strip, otherwise the selected one.
 */
function syncTabStop(strip: HTMLElement): HTMLElement | undefined {
  const tabs = tabListTabs(strip);
  const selected = tabs.find(
    (tab) => tab.getAttribute("aria-selected") === "true",
  );
  const stop = selected ?? tabs[0];
  for (const tab of tabs) tab.tabIndex = tab === stop ? 0 : -1;
  return tabs.find((tab) => tab === document.activeElement) ?? selected;
}

/** Syncs the Tab stop and scrolls the tab the reader is on into view. */
function revealSelectedTab(strip: HTMLElement) {
  const shown = syncTabStop(strip);
  if (shown) revealInStrip(strip, tabRow(shown));
}

export interface ResourceTabStripOptions {
  /** Defaults to automatic: arrowing to a tab also selects it. */
  activation?: TabListActivation;
  /**
   * Identifies the selected tab. When it changes the selected tab is revealed
   * before paint, rather than after the DOM observers report the change.
   */
  revealKey?: string | null;
}

/**
 * A horizontal resource tab list: one Tab stop, arrow-key navigation, and the
 * selected tab kept in view as tabs open, close and the strip resizes. `ref`
 * is the scrolling element with `role="tablist"`.
 */
export function useResourceTabStrip(
  ref: RefObject<HTMLElement | null>,
  { activation = "automatic", revealKey }: ResourceTabStripOptions = {},
) {
  useLayoutEffect(() => {
    const strip = ref.current;
    if (!strip) return;
    const observed = new Set<HTMLElement>();
    // Tabs opened, closed or reselected: track their rows and reveal.
    const reveal = () => {
      const rows = new Set(tabListTabs(strip).map(tabRow));
      for (const row of observed) {
        if (!rows.has(row)) {
          resize?.unobserve(row);
          observed.delete(row);
        }
      }
      for (const row of rows) {
        if (!observed.has(row)) {
          resize?.observe(row);
          observed.add(row);
        }
      }
      revealSelectedTab(strip);
    };
    // A resize reveals only when it can move the tab the reader is on out of
    // view: the strip itself, or that tab's own row. A background tab growing
    // (a status dot, a favicon, a Git mark) must not undo manual scrolling.
    const resize =
      typeof ResizeObserver === "undefined"
        ? null
        : new ResizeObserver((entries) => {
            const shown = syncTabStop(strip);
            const row = shown ? tabRow(shown) : null;
            if (
              row &&
              entries.some(
                (entry) => entry.target === strip || entry.target === row,
              )
            )
              revealInStrip(strip, row);
          });
    resize?.observe(strip);
    reveal();
    const mutation = new MutationObserver((records) => {
      if (
        records.some(
          (record) =>
            record.type === "attributes" ||
            [...record.addedNodes, ...record.removedNodes].some(
              (node) =>
                node instanceof Element &&
                (node.matches('[role="tab"]') ||
                  node.querySelector('[role="tab"]')),
            ),
        )
      )
        reveal();
    });
    mutation.observe(strip, {
      childList: true,
      subtree: true,
      attributes: true,
      attributeFilter: ["aria-selected"],
    });
    return () => {
      resize?.disconnect();
      mutation.disconnect();
    };
  }, [ref]);

  useLayoutEffect(() => {
    if (ref.current) revealSelectedTab(ref.current);
  }, [ref, revealKey]);

  return (event: KeyboardEvent<HTMLElement>) => {
    const strip = ref.current;
    handleTabListKeyDown(event, strip, "horizontal", activation, (tab) => {
      if (strip) revealInStrip(strip, tabRow(tab));
    });
  };
}
