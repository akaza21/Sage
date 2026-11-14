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
// before its own lazy surface mounts, so only `open` can still see it. A weak
// reference, so a view released while Settings is open (a project switch
// unmounts the composer) is not kept alive by it.
let opener: WeakRef<Element> | null = null;

/** Leaves focus alone if it already moved out of `surface`, or on a remount. */
export function returnFocusToAgentSettingsOpener(surface: Element | null) {
  if (useAgentSettingsState.getState().page !== null) return;
  const target = opener?.deref();
  opener = null;
  const active = document.activeElement;
  if (active && active !== document.body && !surface?.contains(active)) return;
  if (target instanceof HTMLElement && target.isConnected) target.focus();
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
  open: (page, browserTabId, showSitePermissions = false) => {
    if (useAgentSettingsState.getState().page === null) {
      const active = document.activeElement;
      opener = active ? new WeakRef(active) : null;
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
