import { create } from "zustand";

import type { AgentArtifactAdmission } from "../domain/agentArtifact";
import {
  admitAgentArtifact,
  forgetAgentArtifactTab,
} from "../workbench/agentArtifactAdmission";

interface AgentArtifactAdmissionStore {
  admissions: readonly AgentArtifactAdmission[];
  admit: (admission: AgentArtifactAdmission) => void;
  forget: (tabId: string) => void;
  clear: () => void;
}

export const useAgentArtifactAdmissionStore =
  create<AgentArtifactAdmissionStore>((set, get) => ({
    admissions: [],
    admit: (admission) =>
      set({ admissions: admitAgentArtifact(get().admissions, admission) }),
    forget: (tabId) =>
      set({ admissions: forgetAgentArtifactTab(get().admissions, tabId) }),
    clear: () => set({ admissions: [] }),
  }));
