import type { StoreApi } from "zustand";
import { agentChatFocus } from "./agentChatOwner";
import { sessionAccessModeCandidates } from "../domain/sessionAccessMode";
import { preferredModelId } from "./harnessSlice";
import type { NewChatDraft } from "./newChatDraftSlice";
import type { WorkbenchState } from "./store";
type Set = StoreApi<WorkbenchState>["setState"];
type Get = StoreApi<WorkbenchState>["getState"];
export async function openNewChatDraft(set: Set, get: Get, projectId?: string) {
  let state = get();
  if (state.projectTransitioning) return;
  if (!state.project) {
    // Initialization supplies an owner, while projectId keeps the requested
    // destination (including the omitted, no-project destination).
    await state.ensureLocalProject();
    state = get();
    if (!state.project || state.projectTransitioning) return;
  }
  // An omitted target reopens the draft. Explicit retargeting may replace an
  // empty draft, but an unsent prompt must be sent or discarded by its owner.
  const draft = state.newChatDraft;
  const retainedDraft =
    draft &&
    (projectId === undefined ||
      (draft.targetProjectId === undefined
        ? draft.projectId
        : draft.targetProjectId) === projectId ||
      draft.text.length > 0 ||
      draft.submission.status !== "released")
      ? draft
      : null;
  const draftFocusRequest =
    (state.draftFocusRequest + 1) % Number.MAX_SAFE_INTEGER;
  if (state.agentCenter === "draft" && retainedDraft) {
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
  set({
    agentCenter: "draft",
    pendingFirstSend: null,
    draftFocusRequest,
    newChatDraft: retainedDraft ?? {
      id: {},
      projectId: state.project?.id ?? null,
      targetProjectId: projectId ?? null,
      text: "",
      harnessId: state.selectedHarnessId,
      modelId: state.selectedModelId,
      access: null,
      effort: null,
      submission: { status: "released" },
      error: null,
    },
  });
}
/**
 * The choices a draft sends with under the current catalog, or null while
 * its harness cannot send. The stored choices stay the user's preference, so
 * a catalog that briefly lacks them does not discard them.
 */
export function effectiveDraftChoice(
  draft: NewChatDraft,
  state: Pick<
    WorkbenchState,
    "harnesses" | "selectedHarnessId" | "harnessDiscoveryPhase" | "models"
  >,
) {
  const harness = state.harnesses.find(
    (harness) => harness.id === draft.harnessId,
  );
  if (
    !harness?.bindingId ||
    harness.installState !== "available" ||
    state.selectedHarnessId !== harness.id ||
    state.harnessDiscoveryPhase !== "ready"
  )
    return null;
  const modelId = preferredModelId(state.models, draft.modelId);
  const levels = state.models.find(
    (model) => model.id === modelId,
  )?.effortLevels;
  return {
    harnessId: harness.id,
    bindingId: harness.bindingId,
    modelId,
    effort:
      draft.effort && levels?.includes(draft.effort) ? draft.effort : null,
    access: sessionAccessModeCandidates(harness.accessModes, true).some(
      (mode) => mode.id === draft.access,
    )
      ? draft.access
      : null,
  };
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
    draft.projectId ?? "",
  );
}
