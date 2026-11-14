import type { StateCreator } from "zustand";
import type { SessionAccessMode } from "../domain/sessionAccessMode";
import type { createNewChatDraftActions } from "./newChatDraftActions";
import type { WorkbenchState } from "./store";

export interface NewChatDraft {
  projectId: string;
  text: string;
  harnessId: string | null;
  modelId: string | null;
  access: SessionAccessMode | null;
  effort: string | null;
  creating: boolean;
  error: string | null;
  unconfirmedAtProjectGeneration?: number;
}

export interface NewChatDraftSlice {
  agentCenter: "chat" | "draft";
  newChatDraft: NewChatDraft | null;
  draftFocusRequest: number;
  pendingFirstSend: {
    projectId: string;
    chatId: string | null;
    ownershipEpoch: number;
  } | null;
  openNewChatDraft: (projectId?: string) => Promise<unknown>;
  updateNewChatDraft: (
    patch: Partial<Pick<NewChatDraft, "text" | "access" | "effort">>,
  ) => void;
  selectDraftHarness: (harnessId: string) => Promise<void>;
  selectDraftModel: (modelId: string) => void;
  sendNewChatDraft: () => Promise<void>;
}

export const createNewChatDraftSlice: StateCreator<
  WorkbenchState,
  [],
  [],
  NewChatDraftSlice
> = (set, get) => {
  // Opening the draft loads its actions first. Once visible, edits and the
  // first-send guard stay synchronous, without carrying them on startup.
  let actions: ReturnType<typeof createNewChatDraftActions> | undefined;
  const load = async () =>
    (actions ??= (
      await import("./newChatDraftActions")
    ).createNewChatDraftActions(set, get));
  return {
    agentCenter: "chat",
    newChatDraft: null,
    draftFocusRequest: 0,
    pendingFirstSend: null,
    openNewChatDraft: async (projectId) => {
      return (await load()).openNewChatDraft(projectId);
    },
    updateNewChatDraft: (patch) => actions?.updateNewChatDraft(patch),
    selectDraftHarness: async (harnessId) => {
      await (await load()).selectDraftHarness(harnessId);
    },
    selectDraftModel: (modelId) => actions?.selectDraftModel(modelId),
    sendNewChatDraft: () => actions?.sendNewChatDraft() ?? Promise.resolve(),
  };
};

export function projectNewChatDraft(
  state: Pick<WorkbenchState, "project">,
  projectId: string | null,
) {
  return state.project?.id === projectId
    ? {}
    : {
        newChatDraft: null,
        agentCenter: "chat" as const,
        pendingFirstSend: null,
      };
}

export function reopenedNewChatDraft(
  state: Pick<WorkbenchState, "project" | "projectGeneration" | "newChatDraft">,
  generation: number,
) {
  const draft = state.newChatDraft;
  if (
    draft?.unconfirmedAtProjectGeneration === undefined ||
    generation <= draft.unconfirmedAtProjectGeneration ||
    state.projectGeneration !== generation ||
    state.project?.id !== draft.projectId
  )
    return {};
  return {
    newChatDraft: {
      ...draft,
      creating: false,
      error: null,
      unconfirmedAtProjectGeneration: undefined,
    },
  };
}
