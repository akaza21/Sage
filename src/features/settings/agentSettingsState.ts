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
  open: (page = "Appearance", browserTabId) =>
    set({ page, browserTabId: browserTabId ?? null }),
  close: () => set({ page: null, browserTabId: null }),
}));
