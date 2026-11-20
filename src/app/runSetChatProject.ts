import type { StoreApi } from "zustand";
import type { WorkbenchState } from "./store";
import type { ProjectLayoutRequest } from "../domain/workbench";
import type { ChatPort } from "../ports/workbenchPorts";
import { currentProjectLayout } from "./projectProjection";
import type { createChatTransitionContext } from "./chatTransitions";
import type { ChatComposerDraft } from "../domain/chatOwnership";

/** Loads only when an unsent chat actually changes its project. */
export async function runSetChatProject(
  get: StoreApi<WorkbenchState>["getState"],
  chat: ChatPort,
  runChatTransition: ReturnType<
    typeof createChatTransitionContext
  >["runChatTransition"],
  targetProjectId: string | null,
  sourceLayout?: ProjectLayoutRequest,
  sourceComposer?: ChatComposerDraft,
) {
  const current = sourceLayout ?? currentProjectLayout(get());
  const chatId = current?.activeTabId;
  if (!current || !chatId || get().projectTransitioning) return;
  const expectedTransitionEpoch = get().nativeTransitionEpoch;
  const entry = sourceComposer ?? get().chatUiById[chatId];
  const composer =
    sourceComposer?.composerText ??
    (get().activeTabId === chatId ? get().composerText : entry?.composerText);
  const outcome = await runChatTransition({
    action: "project selection",
    expectedTransitionEpoch,
    prepare: () => get().flushActiveEditor(),
    invoke: () =>
      chat.setChatProject(
        current,
        expectedTransitionEpoch,
        chatId,
        targetProjectId,
      ),
    classify: (snapshot) =>
      snapshot.activeTabId === chatId &&
      (targetProjectId
        ? snapshot.project?.id === targetProjectId
        : snapshot.project?.isInternal)
        ? "committed"
        : "unchanged",
  });
  if (outcome === "committed" && get().activeTabId === chatId) {
    const owner = get().captureChatOwnership(chatId);
    if (owner) {
      const contexts =
        entry?.attachedContext.filter(
          (context) =>
            context.kind !== "diffLineComment" ||
            context.diffRef.projectId === owner.projectId,
        ) ?? [];
      get().restoreChatComposer(owner, composer ?? "", contexts);
      if (entry)
        get().setChatHarnessSelection(owner, {
          harnessId: entry.harnessId ?? null,
          modelId: entry.modelId ?? null,
        });
      // Effort/access drafts stay keyed by this unchanged Chat id in their
      // existing bounded stores throughout the project transition.
    }
  }
}
