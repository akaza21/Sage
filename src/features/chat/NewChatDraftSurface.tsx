import { useEffect, useRef } from "react";
import { useWorkbenchStore } from "../../app/store";
import { useSessionAccessSelection } from "../../app/useSessionAccessSelection";
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
  const project = useWorkbenchStore((state) => state.project);
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
  const update = useWorkbenchStore((state) => state.updateNewChatDraft);
  const chooseHarness = useWorkbenchStore((state) => state.selectDraftHarness);
  const chooseModel = useWorkbenchStore((state) => state.selectDraftModel);
  const composerRef = useRef<HTMLTextAreaElement>(null);
  useComposerFocusHotkey(composerRef, !draft?.creating && !transitioning);
  const harness = harnesses.find((harness) => harness.id === draft?.harnessId);
  const access = useSessionAccessSelection(
    harness?.bindingId,
    harness?.accessModes,
  );
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
  useEffect(() => {
    if (
      !draft ||
      draft.creating ||
      transitioning ||
      draft.harnessId !== selectedHarnessId ||
      discoveryPhase !== "ready" ||
      models.some((model) => model.id === draft.modelId)
    )
      return;
    const model = models.find((model) => model.isDefault) ?? models[0];
    if (model) chooseModel(model.id);
  }, [
    chooseModel,
    discoveryPhase,
    draft,
    models,
    selectedHarnessId,
    transitioning,
  ]);
  if (!draft || !project) return null;
  const available = harness?.installState === "available";
  const canSend =
    available &&
    !!harness?.bindingId &&
    selectedHarnessId === draft.harnessId &&
    discoveryPhase === "ready" &&
    !!draft.modelId;
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
            <ChatHome projectName={project.name} />
          </div>
        </div>
        <ChatComposerRegion
          sessionFeatures={false}
          composerRef={composerRef}
          fieldId="new-chat-prompt"
          dictationScope=""
          descriptionId="new-chat-description"
          disabledReasonId="new-chat-disabled"
          text={draft.text}
          attachedContext={emptyContext}
          enabled={!draft.creating}
          sendBlocked={!canSend}
          actionBusy={draft.creating}
          activeTurn={false}
          projectTransitioning={transitioning}
          workbenchError={null}
          chatError={draft.error}
          accessTarget={{ bindingId: harness?.bindingId }}
          draftPickers={{
            access: draft.access ?? access.effective,
            effort: draft.effort,
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
          selectedModelId={draft.modelId}
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
          project={project}
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
