import type { StoreApi } from "zustand";
import { agentChatFocus } from "./agentChatOwner";
import { reconcileDraftModel } from "./harnessSlice";
import type { WorkbenchState } from "./store";
type Set = StoreApi<WorkbenchState>["setState"];
type Get = StoreApi<WorkbenchState>["getState"];
export function openNewChatDraft(set: Set, get: Get, projectId?: string) {
  const state = get();
  if (projectId && projectId !== state.project?.id)
    return state.createChat(projectId);
  if (state.projectTransitioning) return;
  if (!state.project) return state.createChat();
  const draftFocusRequest =
    (state.draftFocusRequest + 1) % Number.MAX_SAFE_INTEGER;
  if (state.agentCenter === "draft" && state.newChatDraft) {
    set({ draftFocusRequest });
    return;
  }
  const intent =
    state.captureChatOwnership() ??
    state.captureChatOwnership(agentChatFocus() ?? undefined);
  if (intent && !state.chatUiById[intent.chatId]?.harnessId) {
    state.setChatHarnessSelection(intent, {
      harnessId: state.selectedHarnessId,
      modelId: state.selectedModelId,
    });
  }
  const draft = state.newChatDraft ?? {
    id: {},
    projectId: state.project.id,
    text: "",
    harnessId: state.selectedHarnessId,
    modelId: state.selectedModelId,
    access: null,
    effort: null,
    submission: { status: "released" as const },
    error: null,
  };
  set({
    agentCenter: "draft",
    pendingFirstSend: null,
    draftFocusRequest,
    newChatDraft:
      state.harnessDiscoveryPhase === "ready"
        ? reconcileDraftModel(draft, state.selectedHarnessId, state.models)
        : draft,
  });
}
export async function selectDraftHarness(
  set: Set,
  get: Get,
  harnessId: string,
) {
  const state = get();
  const draft = state.newChatDraft;
  if (
    !draft ||
    draft.submission.status === "submitting" ||
    state.projectTransitioning
  )
    return;
  const changed = draft.harnessId !== harnessId;
  if (changed)
    set({
      newChatDraft: {
        ...draft,
        harnessId,
        modelId: null,
        access: null,
        effort: null,
        error: null,
      },
    });
  await state.loadHarnessModelsForProjection(
    harnessId,
    changed ? null : draft.modelId,
    state.projectGeneration,
    draft.projectId,
  );
}
