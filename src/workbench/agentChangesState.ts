import { create } from "zustand";
export type ChangesOwner = {
  projectId: string;
  chatId: string;
  sessionId: string | null;
};

export const useAgentChangesState = create<{
  owner: ChangesOwner | null;
  generation: number;
  selected: boolean;
  show: (owner: ChangesOwner, generation: number) => void;
  select: (selected: boolean) => void;
  close: () => void;
}>((set) => ({
  owner: null,
  generation: -1,
  selected: false,
  show: (owner, generation) => set({ owner, generation, selected: true }),
  select: (selected) => set({ selected }),
  close: () => set({ owner: null, selected: false }),
}));
