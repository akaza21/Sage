import { create } from "zustand";
import { useAgentArtifactFollowStore } from "../app/agentArtifactFollowStore";
import type { AgentPaneOwner } from "../domain/agentArtifact";
export type ChangesOwner = AgentPaneOwner;

export const useAgentChangesState = create<{
  owner: ChangesOwner | null;
  generation: number;
  show: (
    owner: ChangesOwner,
    generation: number,
    mode?: "follow" | "pinned",
  ) => void;
  close: () => void;
}>((set, get) => ({
  owner: null,
  generation: -1,
  show: (owner, generation, mode = "pinned") => {
    set({ owner, generation });
    const follow = useAgentArtifactFollowStore.getState();
    if (mode === "follow")
      follow.observe(owner, { kind: "changes", generation });
    else follow.pin(owner, { kind: "changes", generation });
  },
  close: () => {
    const owner = get().owner;
    if (owner) {
      const follow = useAgentArtifactFollowStore.getState();
      follow.dismiss(owner, { kind: "changes" });
    }
    set({ owner: null });
  },
}));
