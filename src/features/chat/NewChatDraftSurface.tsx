import { useEffect, useRef } from "react";
import { effectiveDraftChoice } from "../../app/newChatDraftNavigation";
import { draftOwnedChatId } from "../../app/newChatDraftSlice";
import { useWorkbenchStore } from "../../app/store";
import { AgentNavigationPart } from "../../workbench/agentNavigationLazy";
import { ChatHome } from "./ChatHome";
import { useComposerFocusHotkey } from "./composerChrome";
import { composerProblem } from "./ComposerStatusNotices";
import { ChatComposerRegion } from "./ChatComposerRegion";
import { preferredAvailableHarness } from "../../domain/harness";

const unchangedSession = () => Promise.resolve();
// Images taken just before Send join the draft before it is sent: Send waits
// for every drop still in flight, and with none acts at once.
let intake: Promise<unknown> | undefined;

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
  const surfaceRef = useRef<HTMLElement>(null);
  const submitting = draft?.submission.status === "submitting";
  const ownsChat = useWorkbenchStore((state) => !!draftOwnedChatId(state));
  const unconfirmed =
    draft?.submission.status === "unconfirmed" ||
    (draft?.submission.status === "submitting" &&
      draft.submission.creation.status === "unconfirmed");
  useComposerFocusHotkey(composerRef, !submitting && !transitioning);
  const harness = harnesses.find((harness) => harness.id === draft?.harnessId);
  useEffect(() => {
    (composerRef.current?.disabled
      ? surfaceRef.current
      : (composerRef.current ?? surfaceRef.current)
    )?.focus();
  }, [focusRequest]);
  useEffect(() => {
    const available = preferredAvailableHarness(harnesses, draft?.harnessId);
    if (
      draft &&
      draft.submission.status === "released" &&
      !transitioning &&
      available &&
      draft.harnessId !== available.id
    ) {
      void chooseHarness(available.id);
      return;
    }
    if (
      draft &&
      !transitioning &&
      selectedHarnessId !== draft.harnessId &&
      draft.harnessId
    ) {
      void chooseHarness(draft.harnessId);
    }
  }, [chooseHarness, draft, harnesses, selectedHarnessId, transitioning]);
  if (!draft) return null;
  const targetId =
    draft.targetProjectId === undefined
      ? draft.projectId
      : draft.targetProjectId;
  // Only its name is shown.
  const shownProject = projects.find((item) => item.id === targetId);
  const available = harness?.installState === "available";
  const choice = effectiveDraftChoice(draft, {
    harnesses,
    selectedHarnessId,
    harnessDiscoveryPhase: discoveryPhase,
    models,
  });
  const errorNotice =
    draft.error || unconfirmed ? (
      <>
        {draft.error ??
          "Chat creation is unconfirmed. Reload chats before trying again."}
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
    ) : null;
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
        ref={surfaceRef}
        tabIndex={-1}
        className="chat-surface chat-surface--agent"
        aria-label="New chat draft"
      >
        <div className="message-list message-list--home">
          <div className="message-list__content">
            <ChatHome projectName={shownProject?.name} />
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
          attachedContext={draft.images ?? []}
          // Images go to a chat's native session, so only with a binding.
          imageInputAvailable={!!harness?.bindingId}
          onAddImages={
            harness?.bindingId
              ? (files) => {
                  // Each drop joins after the one before it, in drop order.
                  const next: Promise<unknown> = Promise.all([
                    intake,
                    import("./ComposerSectionMenu"),
                  ])
                    .then(([, images]) =>
                      images.addDraftImages(draft.id, files),
                    )
                    .finally(() => {
                      if (intake === next) intake = undefined;
                    });
                  intake = next;
                }
              : undefined
          }
          enabled={!submitting && available}
          sendBlocked={!choice || unconfirmed || !draft.text.trim()}
          actionBusy={submitting}
          activeTurn={false}
          projectTransitioning={transitioning}
          chatError={composerProblem("create-chat", "new-chat", errorNotice)}
          accessTarget={{ bindingId: harness?.bindingId }}
          draftPickers={{
            access: (choice ?? draft).access,
            effort: (choice ?? draft).effort,
            onAccessChange: (access) => update({ access }),
            onEffortChange: (effort) =>
              update({ effort: effort === "default" ? null : effort }),
          }}
          availableBinding={!!harness?.bindingId}
          canPrompt={available}
          discoveryPhase={discoveryPhase}
          selectedHarnessId={draft.harnessId}
          selectedModelId={(choice ?? draft).modelId}
          sessionConfig={null}
          modePending={false}
          focused
          onSetSessionMode={unchangedSession}
          onSetSessionConfigOption={unchangedSession}
          onSelectHarness={chooseHarness}
          onSelectModel={chooseModel}
          missingBindingNotice={!available}
          disabledReason={
            available ? null : "Choose an available harness before sending."
          }
          project={shownProject}
          newChat
          onSubmit={(event) => {
            event.preventDefault();
            const send = useWorkbenchStore.getState().sendNewChatDraft;
            void (intake?.finally(send) ?? send());
          }}
          onTextChange={(text) => update({ text })}
          onRemoveContext={(context) =>
            update({
              images: draft.images?.filter((image) => image !== context),
            })
          }
        />
      </section>
    </div>
  );
}
