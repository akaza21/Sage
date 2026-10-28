import { create } from "zustand";

export type AgentSettingsPage =
  | "General"
  | "Harnesses"
  | "Browser"
  | "Keyboard shortcuts"
  | "Archived chats"
  | "Appearance"
  | "Notifications"
  | "Approvals";

export const useAgentSettingsState = create<{
  page: AgentSettingsPage | null;
  browserTabId: string | null;
  open: (page?: AgentSettingsPage, browserTabId?: string) => void;
  close: () => void;
}>((set) => ({
  page: null,
  browserTabId: null,
  open: (page = "General", browserTabId) =>
    set({ page, browserTabId: browserTabId ?? null }),
  close: () => set({ page: null, browserTabId: null }),
}));
