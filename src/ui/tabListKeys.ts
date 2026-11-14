import type { KeyboardEvent } from "react";

import {
  hasNavigationModifier,
  listIndexForKey,
  type ListOrientation,
} from "./listNavigation";

export type TabListOrientation = ListOrientation;

export type TabListActivation = "automatic" | "manual";

export function tabIndexForKey(
  key: string,
  index: number,
  disabled: readonly boolean[],
  orientation: TabListOrientation,
): number | null {
  return listIndexForKey(key, index, disabled, { orientation, wrap: true });
}

export function tabListTabs(list: HTMLElement | null): HTMLElement[] {
  return Array.from(list?.querySelectorAll<HTMLElement>('[role="tab"]') ?? []);
}

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
