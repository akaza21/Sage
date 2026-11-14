import type { StateCreator } from "zustand";
import type { SessionAccessMode } from "../domain/sessionAccessMode";
import type { createNewChatDraftActions } from "./newChatDraftActions";
import type { CreateChatResult } from "./runCreateChat";
import type { WorkbenchState } from "./store";

export type DraftCreation =
  | { status: "released" }
  | { status: "created"; chatId: string }
  | { status: "unconfirmed"; reconcile: () => Promise<CreateChatResult> };

export interface NewChatDraft {
  /** Stable across immutable edits and project ownership recovery. */
  id: object;
  projectId: string;
  text: string;
  harnessId: string | null;
  modelId: string | null;
  access: SessionAccessMode | null;
  effort: string | null;
  submission: DraftCreation | { status: "submitting"; creation: DraftCreation };
  error: string | null;
}

export interface NewChatDraftSlice {
  agentCenter: "chat" | "draft";
  newChatDraft: NewChatDraft | null;
  draftFocusRequest: number;
  pendingFirstSend: {
    projectId: string;
    chatId: string | null;
    ownershipEpoch: number;
    /** The chat's edit revision once handed the prompt; an edit cancels. */
    editRevision?: number;
  } | null;
  openNewChatDraft: (projectId?: string) => Promise<unknown>;
  updateNewChatDraft: (
    patch: Partial<Pick<NewChatDraft, "text" | "access" | "effort">>,
  ) => void;
  selectDraftHarness: (harnessId: string) => Promise<void>;
  selectDraftModel: (modelId: string) => void;
  sendNewChatDraft: () => Promise<void>;
  reloadNewChatDraft: () => Promise<void>;
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
    reloadNewChatDraft: () =>
      actions?.reloadNewChatDraft() ?? Promise.resolve(),
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
