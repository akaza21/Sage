import { create } from "zustand";

import type { SettingsPageId } from "../domain/settingsNavigation";

/** Type-only import: the page catalogue stays out of startup. */
export const useSettingsPageRequest = create<{ page: SettingsPageId | null }>(
  () => ({ page: null }),
);

export function requestSettingsPage(page: SettingsPageId | undefined): void {
  if (page) useSettingsPageRequest.setState({ page });
}
