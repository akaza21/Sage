import type { StateCreator } from "zustand";
import type { SessionAccessMode } from "../domain/sessionAccessMode";
import type { createNewChatDraftActions } from "./newChatDraftActions";
import type { CreateChatResult } from "./runCreateChat";
import type { WorkbenchState } from "./store";

export type DraftCreation =
  | { status: "released" }
  | { status: "created"; chatId: string }
  | {
      status: "unconfirmed";
      chatId: string;
      reconcile: () => Promise<CreateChatResult>;
    };

export interface NewChatDraft {
  /** Stable across immutable edits and project ownership recovery. */
  id: object;
  projectId: string | null;
  /** Undefined retains the owner project; null explicitly chooses no project. */
  targetProjectId?: string | null;
  text: string;
  harnessId: string | null;
  modelId: string | null;
  access: SessionAccessMode | null;
  effort: string | null;
  submission:
    | DraftCreation
    | { status: "submitting"; chatId: string; creation: DraftCreation };
  error: string | null;
}

export interface NewChatDraftOpenOptions {
  /** Automatic continuity must not supersede a later explicit navigation. */
  requestFocus?: boolean;
}

interface DraftOpenIntent {
  projectId?: string;
  sourceProjectId: string | null;
  activeTabId: string | null;
  ownershipEpoch: number;
}

export interface NewChatDraftSlice {
  agentCenter: "chat" | "draft";
  newChatDraft: NewChatDraft | null;
  draftFocusRequest: number;
  newChatDraftOpening: boolean;
  /** Cancellation permits one continuity open; failure requires user retry. */
  newChatDraftOpenOutcome: "cancelled" | "failed" | null;
  /** A first send waiting for its chat, then for that chat's page. */
  pendingFirstSend:
    | { projectId: string | null; chatId: null; ownershipEpoch: number }
    | {
        projectId: string;
        chatId: string;
        ownershipEpoch: number;
        /** The text handed to the chat; a different composer text cancels. */
        text: string;
      }
    | null;
  openNewChatDraft: (
    projectId?: string,
    options?: NewChatDraftOpenOptions,
  ) => Promise<unknown>;
  updateNewChatDraft: (
    patch: Partial<
      Pick<NewChatDraft, "text" | "access" | "effort" | "targetProjectId">
    >,
  ) => void;
  selectDraftHarness: (harnessId: string) => Promise<void>;
  selectDraftModel: (modelId: string) => void;
  sendNewChatDraft: () => Promise<void>;
  reloadNewChatDraft: () => Promise<void>;
  discardNewChatDraft: () => void;
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
  // Coalesce repeated explicit opens into one latest intent while loading.
  let queuedOpen: DraftOpenIntent | null = null;
  const load = async () =>
    (actions ??= (
      await import("./newChatDraftActions")
    ).createNewChatDraftActions(set, get));
  return {
    agentCenter: "chat",
    newChatDraft: null,
    draftFocusRequest: 0,
    newChatDraftOpening: false,
    newChatDraftOpenOutcome: null,
    pendingFirstSend: null,
    openNewChatDraft: async (projectId, options) => {
      const from = get();
      let request: DraftOpenIntent | null = {
        projectId,
        sourceProjectId: from.project?.id ?? null,
        activeTabId: from.activeTabId,
        ownershipEpoch: from.chatOwnershipEpoch,
      };
      const explicit = options?.requestFocus !== false;
      if (explicit)
        set({
          draftFocusRequest:
            (from.draftFocusRequest + 1) % Number.MAX_SAFE_INTEGER,
        });
      if (from.newChatDraftOpening) {
        if (explicit) queuedOpen = request;
        return;
      }
      set({ newChatDraftOpening: true, newChatDraftOpenOutcome: null });
      try {
        do {
          queuedOpen = null;
          let outcome: NewChatDraftSlice["newChatDraftOpenOutcome"] = null;
          try {
            const loaded = await load();
            const now = get();
            if (
              (now.project?.id ?? null) === request.sourceProjectId &&
              now.activeTabId === request.activeTabId &&
              now.chatOwnershipEpoch === request.ownershipEpoch
            )
              await loaded.openNewChatDraft(request.projectId);
            else outcome = "cancelled";
          } catch {
            outcome = "failed";
          }
          set({ newChatDraftOpenOutcome: outcome });
          request = queuedOpen;
        } while (request);
      } finally {
        queuedOpen = null;
        set({ newChatDraftOpening: false });
      }
    },
    updateNewChatDraft: (patch) => actions?.updateNewChatDraft(patch),
    selectDraftHarness: async (harnessId) =>
      (await load()).selectDraftHarness(harnessId),
    selectDraftModel: (modelId) => actions?.selectDraftModel(modelId),
    sendNewChatDraft: async () => actions?.sendNewChatDraft(),
    reloadNewChatDraft: async () => actions?.reloadNewChatDraft(),
    discardNewChatDraft: () => actions?.discardNewChatDraft(),
  };
};

type DraftOwnerState = Pick<
  WorkbenchState,
  "newChatDraft" | "project" | "tabs" | "archivedChats"
>;

/**
 * The draft's creation, released once the chat it created no longer exists:
 * a deleted chat cannot take the prompt, so the next send creates another.
 */
export function liveDraftCreation(
  creation: DraftCreation,
  state: Pick<WorkbenchState, "tabs" | "archivedChats">,
): DraftCreation {
  return creation.status === "created" &&
    !state.tabs.some((tab) => tab.id === creation.chatId) &&
    !state.archivedChats.some((chat) => chat.id === creation.chatId)
    ? { status: "released" }
    : creation;
}

/**
 * The chat a draft is creating or has created. Only the draft's handoff or
 * an explicit discard releases it; until then the draft is shown wherever
 * that chat would be, and no other writer owns it.
 */
export function draftOwnedChatId(state: DraftOwnerState) {
  const draft = state.newChatDraft;
  if (!draft || draft.projectId !== state.project?.id) return null;
  const submission =
    draft.submission.status === "submitting"
      ? draft.submission
      : liveDraftCreation(draft.submission, state);
  return submission.status === "released" ? null : submission.chatId;
}

export function draftShownFor(
  state: DraftOwnerState & Pick<WorkbenchState, "agentCenter">,
  shownChatId: string | null,
) {
  return (
    state.agentCenter === "draft" ||
    (shownChatId !== null && shownChatId === draftOwnedChatId(state))
  );
}

export function projectNewChatDraft(
  state: Pick<WorkbenchState, "project" | "newChatDraft" | "pendingFirstSend">,
  projectId: string | null,
  createdChatId?: string | null,
) {
  const draft = state.newChatDraft;
  if (
    draft?.submission.status === "submitting" &&
    draft.submission.chatId === createdChatId
  ) {
    return { newChatDraft: { ...draft, projectId } };
  }
  return (state.project?.id ?? null) === projectId
    ? {}
    : {
        newChatDraft: null,
        agentCenter: "chat" as const,
        pendingFirstSend: null,
      };
}
