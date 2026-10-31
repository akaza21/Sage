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
  launcher: HTMLElement | null;
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
  launcher: null,
  // Without a page, keeps an open Settings where it is.
  open: (page, browserTabId, showSitePermissions = false) =>
    set((state) => ({
      ...(!state.page
        ? {
            launcher:
              document.activeElement instanceof HTMLElement
                ? document.activeElement
                : null,
          }
        : {}),
      ...(page
        ? { page, browserTabId: browserTabId ?? null, showSitePermissions }
        : { page: state.page ?? "Appearance" }),
    })),
  close: () =>
    set({ page: null, browserTabId: null, showSitePermissions: false }),
}));
