import type { KeyboardEvent } from "react";

import {
  hasNavigationModifier,
  listIndexForKey,
  type ListOrientation,
} from "./listNavigation";

/** Which arrow keys move between tabs, following the WAI-ARIA tabs pattern. */
export type TabListOrientation = ListOrientation;

/**
 * Whether moving focus also selects the tab. Use "manual" when selecting a
 * tab is costly or navigates away, so arrowing past it must not select it.
 */
export type TabListActivation = "automatic" | "manual";

/**
 * The tab a key moves to from `index`: the arrows for the orientation step
 * and wrap, Home and End go to the first and last enabled tab, and disabled
 * tabs are passed over. Returns null for any other key, or when no tab can
 * take focus.
 */
export function tabIndexForKey(
  key: string,
  index: number,
  disabled: readonly boolean[],
  orientation: TabListOrientation,
): number | null {
  return listIndexForKey(key, index, disabled, { orientation, wrap: true });
}

/** The tabs a tab list owns, in document order. */
export function tabListTabs(list: HTMLElement | null): HTMLElement[] {
  return Array.from(list?.querySelectorAll<HTMLElement>('[role="tab"]') ?? []);
}

/**
 * Keyboard navigation for a tab list. Moves focus to the tab the key names
 * and, with automatic activation, selects it by clicking it. A list that
 * scrolls itself passes `reveal`; focus then never scrolls the page.
 */
export function handleTabListKeyDown(
  event: KeyboardEvent<HTMLElement>,
  list: HTMLElement | null,
  orientation: TabListOrientation,
  activation: TabListActivation,
  reveal?: (tab: HTMLElement) => void,
): void {
  if (hasNavigationModifier(event)) return;
  const target = event.target;
  if (!(target instanceof HTMLElement) || target.getAttribute("role") !== "tab")
    return;
  const tabs = tabListTabs(list);
  const index = tabs.indexOf(target);
  if (index < 0) return;
  const next = tabIndexForKey(
    event.key,
    index,
    tabs.map(
      (tab) =>
        tab.hasAttribute("disabled") ||
        tab.getAttribute("aria-disabled") === "true",
    ),
    orientation,
  );
  if (next === null) return;
  event.preventDefault();
  event.stopPropagation();
  if (next === index) return;
  const tab = tabs[next];
  if (reveal) {
    tab.focus({ preventScroll: true });
    reveal(tab);
  } else tab.focus();
  if (activation === "automatic") tab.click();
}
