import { create } from "zustand";

/** What the Agent sidebar panel beside the icon rail lists. Disposable
 *  rendering state: a reload starts on Home. */
export type AgentPanelView =
  "home" | "recent" | "projects" | "archived" | "harnesses";

export const useAgentPanelView = create<{
  view: AgentPanelView;
  show: (view: AgentPanelView) => void;
}>((set) => ({
  view: "home",
  show: (view) => set({ view }),
}));
