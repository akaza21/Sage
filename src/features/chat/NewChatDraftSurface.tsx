import { useEffect, useRef } from "react";
import { effectiveDraftChoice } from "../../app/newChatDraftNavigation";
import { draftOwnedChatId } from "../../app/newChatDraftSlice";
import { useWorkbenchStore } from "../../app/store";
import { AgentNavigationPart } from "../../workbench/agentNavigationLazy";
import { ChatHome } from "./ChatHome";
import { useComposerFocusHotkey } from "./composerChrome";
import { ChatComposerRegion } from "./ChatComposerRegion";
import { LazyHarnessNotice } from "./lazyParts";

const emptyContext = [] as const;
const unchangedSession = () => Promise.resolve();

export function NewChatDraftSurface({
  navigationCollapsed = false,
}: {
  navigationCollapsed?: boolean;
}) {
  const draft = useWorkbenchStore((state) => state.newChatDraft);
  const focusRequest = useWorkbenchStore((state) => state.draftFocusRequest);
  const projects = useWorkbenchStore((state) => state.projects);
  const harnesses = useWorkbenchStore((state) => state.harnesses);
  const selectedHarnessId = useWorkbenchStore(
    (state) => state.selectedHarnessId,
  );
  const models = useWorkbenchStore((state) => state.models);
  const discoveryPhase = useWorkbenchStore(
    (state) => state.harnessDiscoveryPhase,
  );
  const transitioning = useWorkbenchStore(
    (state) => state.projectTransitioning,
  );
  const pendingFirstSend = useWorkbenchStore((state) => state.pendingFirstSend);
  const reload = useWorkbenchStore((state) => state.reloadNewChatDraft);
  const discard = useWorkbenchStore((state) => state.discardNewChatDraft);
  const update = useWorkbenchStore((state) => state.updateNewChatDraft);
  const chooseHarness = useWorkbenchStore((state) => state.selectDraftHarness);
  const chooseModel = useWorkbenchStore((state) => state.selectDraftModel);
  const composerRef = useRef<HTMLTextAreaElement>(null);
  const submitting = draft?.submission.status === "submitting";
  const ownsChat = useWorkbenchStore((state) => !!draftOwnedChatId(state));
  const unconfirmed =
    draft?.submission.status === "unconfirmed" ||
    (draft?.submission.status === "submitting" &&
      draft.submission.creation.status === "unconfirmed");
  useComposerFocusHotkey(composerRef, !submitting && !transitioning);
  const harness = harnesses.find((harness) => harness.id === draft?.harnessId);
  useEffect(() => {
    composerRef.current?.focus();
  }, [focusRequest]);
  useEffect(() => {
    if (
      draft &&
      !transitioning &&
      selectedHarnessId !== draft.harnessId &&
      draft.harnessId
    ) {
      void chooseHarness(draft.harnessId);
    }
  }, [chooseHarness, draft, selectedHarnessId, transitioning]);
  if (!draft) return null;
  const targetId =
    draft.targetProjectId === undefined
      ? draft.projectId
      : draft.targetProjectId;
  const targetProject = targetId
    ? projects.find((item) => item.id === targetId)
    : null;
  const shownProject = targetProject
    ? { id: targetProject.id, name: targetProject.name, roots: [] }
    : null;
  const available = harness?.installState === "available";
  const choice = effectiveDraftChoice(draft, {
    harnesses,
    selectedHarnessId,
    harnessDiscoveryPhase: discoveryPhase,
    models,
  });
  return (
    <div
      className="agent-chat-center surface surface--card"
      role="tabpanel"
      data-new-chat-draft
    >
      {navigationCollapsed ? (
        <div className="agent-chat-center__header">
          <div className="agent-task-header">
            <AgentNavigationPart part="AgentNewChatButton" />
          </div>
        </div>
      ) : null}
      <section
        className="chat-surface chat-surface--agent"
        aria-label="New chat draft"
      >
        <LazyHarnessNotice
          harness={harness ?? null}
          discoveryPhase={discoveryPhase}
        />
        <div className="message-list message-list--home">
          <div className="message-list__content">
            <ChatHome projectName={shownProject?.name ?? null} />
            {ownsChat && !submitting ? (
              <div>
                <button
                  type="button"
                  className="button button--compact"
                  aria-describedby="new-chat-discard-note"
                  disabled={transitioning}
                  onClick={discard}
                >
                  Discard prompt
                </button>
                <span
                  id="new-chat-discard-note"
                  className="select-control__reason"
                >
                  The chat it created stays in your chats.
                </span>
              </div>
            ) : null}
          </div>
        </div>
        <ChatComposerRegion
          composerRef={composerRef}
          fieldId="new-chat-prompt"
          dictationScope=""
          descriptionId="new-chat-description"
          disabledReasonId="new-chat-disabled"
          text={draft.text}
          attachedContext={emptyContext}
          enabled={!submitting}
          sendBlocked={!choice || unconfirmed}
          actionBusy={submitting}
          activeTurn={false}
          projectTransitioning={transitioning}
          workbenchError={null}
          chatError={
            draft.error || unconfirmed ? (
              <>
                {draft.error}
                {unconfirmed ? (
                  <button
                    type="button"
                    className="button button--compact"
                    disabled={transitioning || !!pendingFirstSend}
                    onClick={() => void reload()}
                  >
                    Reload chats
                  </button>
                ) : null}
              </>
            ) : null
          }
          accessTarget={{ bindingId: harness?.bindingId }}
          draftPickers={{
            access: choice ? choice.access : draft.access,
            effort: choice ? choice.effort : draft.effort,
            onAccessChange: (access) => update({ access }),
            onEffortChange: (effort) =>
              update({ effort: effort === "default" ? null : effort }),
          }}
          availableBinding={!!harness?.bindingId}
          canPrompt={!!available}
          discoveryPhase={discoveryPhase}
          sessionHarnessId={null}
          sessionModelId={null}
          selectedHarnessId={draft.harnessId}
          selectedModelId={choice ? choice.modelId : draft.modelId}
          sessionPromptUnavailable={false}
          sessionConfig={null}
          modePending={false}
          modeError={null}
          focused
          onSetSessionMode={unchangedSession}
          onSetSessionConfigOption={unchangedSession}
          onSelectHarness={chooseHarness}
          onSelectModel={chooseModel}
          missingBindingNotice={false}
          disabledReason={null}
          project={shownProject}
          newChat
          usage={null}
          goal={{ kind: "unsupported" }}
          onSubmit={(event) => {
            event.preventDefault();
            void useWorkbenchStore.getState().sendNewChatDraft();
          }}
          onTextChange={(text) => update({ text })}
          onRemoveContext={() => undefined}
          onCancelTurn={null}
        />
      </section>
    </div>
  );
}
