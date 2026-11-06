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
  /** Counts explicit open requests; adopting a session is not one. */
  request: number;
  show: (owner: ChangesOwner, generation: number) => void;
  /** Gives a sessionless Changes tab the session its Chat just started. */
  adopt: (owner: ChangesOwner) => void;
  select: (selected: boolean) => void;
  close: () => void;
}>((set) => ({
  owner: null,
  generation: -1,
  selected: false,
  request: 0,
  show: (owner, generation) =>
    set((state) => ({
      owner,
      generation,
      selected: true,
      request: state.request + 1,
    })),
  adopt: (owner) =>
    set((state) =>
      state.owner?.sessionId === null &&
      state.owner.projectId === owner.projectId &&
      state.owner.chatId === owner.chatId
        ? { owner }
        : state,
    ),
  select: (selected) => set({ selected }),
  close: () => set({ owner: null, selected: false }),
}));
