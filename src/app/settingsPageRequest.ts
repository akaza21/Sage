import { create } from "zustand";

import type { SettingsPageId } from "../domain/settingsNavigation";

/**
 * The Settings page a caller asked to open, such as Browser from the browser's
 * menu. The Settings surface takes it when it mounts or while it is open, then
 * clears it. Type-only import: the page catalogue stays out of startup.
 */
export const useSettingsPageRequest = create<{ page: SettingsPageId | null }>(
  () => ({ page: null }),
);

export function requestSettingsPage(page: SettingsPageId | undefined): void {
  if (page) useSettingsPageRequest.setState({ page });
}
