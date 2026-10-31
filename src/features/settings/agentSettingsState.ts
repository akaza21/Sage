import { create } from "zustand";

export type AgentSettingsPage =
  | "Appearance"
  | "Keybindings"
  | "Harnesses"
  | "Chats"
  | "Browser"
  | "Archived chats";

export const useAgentSettingsState = create<{
  page: AgentSettingsPage | null;
  browserTabId: string | null;
  showSitePermissions: boolean;
  open: (
    page?: AgentSettingsPage,
    browserTabId?: string,
    showSitePermissions?: boolean,
  ) => void;
  close: () => void;
}>((set) => ({
  page: null,
  browserTabId: null,
  showSitePermissions: false,
  // Without a page, keeps an open Settings where it is.
  open: (page, browserTabId, showSitePermissions = false) =>
    set((state) =>
      page
        ? { page, browserTabId: browserTabId ?? null, showSitePermissions }
        : { page: state.page ?? "Appearance" },
    ),
  close: () =>
    set({ page: null, browserTabId: null, showSitePermissions: false }),
}));
