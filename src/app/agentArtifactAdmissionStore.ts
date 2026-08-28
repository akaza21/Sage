import { create } from "zustand";

import { admitAgentArtifactIds } from "../workbench/agentArtifactAdmission";

interface AgentArtifactAdmissionStore {
  ids: readonly string[];
  admit: (tabId: string) => void;
  clear: () => void;
}

export const useAgentArtifactAdmissionStore =
  create<AgentArtifactAdmissionStore>((set, get) => ({
    ids: [],
    admit: (tabId) => set({ ids: admitAgentArtifactIds(get().ids, tabId) }),
    clear: () => set({ ids: [] }),
  }));
