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
  open: (page?: AgentSettingsPage, browserTabId?: string) => void;
  close: () => void;
}>((set) => ({
  page: null,
  browserTabId: null,
  // Without a page, keeps an open Settings where it is.
  open: (page, browserTabId) =>
    set((state) =>
      page
        ? { page, browserTabId: browserTabId ?? null }
        : { page: state.page ?? "Appearance" },
    ),
  close: () => set({ page: null, browserTabId: null }),
}));
