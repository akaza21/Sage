import { create } from "zustand";

export type AgentSettingsPage =
  | "Appearance"
  | "Keybindings"
  | "Harnesses"
  | "Chats"
  | "Browser"
  | "Passwords"
  | "Archived chats";

// What had focus when Settings opened. Settings hides the Agent workspace
// before its own lazy surface mounts, so only `open` can still see it.
let opener: Element | null = null;

/**
 * After Settings closes, returns focus to what opened it, unless focus has
 * already moved on from `surface` (the closed Settings) to something else.
 * An unmount that is not a close (a remount) leaves focus alone.
 */
export function returnFocusToAgentSettingsOpener(surface: Element | null) {
  if (useAgentSettingsState.getState().page !== null) return;
  const active = document.activeElement;
  if (active && active !== document.body && !surface?.contains(active)) return;
  if (opener instanceof HTMLElement && opener.isConnected) opener.focus();
}

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
  open: (page, browserTabId, showSitePermissions = false) => {
    if (useAgentSettingsState.getState().page === null) {
      opener = document.activeElement;
    }
    set((state) =>
      page
        ? { page, browserTabId: browserTabId ?? null, showSitePermissions }
        : { page: state.page ?? "Appearance" },
    );
  },
  close: () =>
    set({ page: null, browserTabId: null, showSitePermissions: false }),
}));
