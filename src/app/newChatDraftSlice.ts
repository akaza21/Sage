import type { StateCreator } from "zustand";
import {
  boundedChatError,
  validateChatComposerText,
} from "../domain/chatOwnership";
import type { SessionAccessMode } from "../domain/sessionAccessMode";
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
> = (set, get) => ({
  agentCenter: "chat",
  newChatDraft: null,
  draftFocusRequest: 0,
  pendingFirstSend: null,
  openNewChatDraft: async (projectId) => {
    const { openNewChatDraft } = await import("./newChatDraftNavigation");
    return openNewChatDraft(set, get, projectId);
  },
  updateNewChatDraft: (patch) => {
    const draft = get().newChatDraft;
    if (!draft || draft.creating) return;
    try {
      if (patch.text !== undefined) validateChatComposerText(patch.text);
      set({ newChatDraft: { ...draft, ...patch, error: null } });
    } catch (error) {
      set({
        newChatDraft: { ...draft, error: boundedChatError(String(error)) },
      });
    }
  },
  selectDraftHarness: async (harnessId) => {
    const { selectDraftHarness } = await import("./newChatDraftNavigation");
    await selectDraftHarness(set, get, harnessId);
  },
  selectDraftModel: (modelId) => {
    const state = get();
    const draft = state.newChatDraft;
    if (!draft || draft.creating || state.selectedHarnessId !== draft.harnessId)
      return;
    const model = state.models.find((model) => model.id === modelId);
    if (!model) return;
    set({
      newChatDraft: {
        ...draft,
        modelId,
        effort:
          draft.effort && model.effortLevels?.includes(draft.effort)
            ? draft.effort
            : null,
      },
    });
  },
  sendNewChatDraft: async () => {
    const draft = get().newChatDraft;
    if (
      !draft ||
      draft.creating ||
      !draft.text.trim() ||
      get().projectTransitioning
    )
      return;
    const submission = {
      projectId: draft.projectId,
      chatId: null,
      ownershipEpoch: get().chatOwnershipEpoch,
    };
    set({
      newChatDraft: { ...draft, creating: true, error: null },
      pendingFirstSend: submission,
    });
    const { runNewChatDraftSend } = await import("./runNewChatDraftSend");
    await runNewChatDraftSend(set, get, draft, submission);
  },
});

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
