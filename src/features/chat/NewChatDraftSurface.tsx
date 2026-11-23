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
import { LazyHarnessNotice } from "./lazyParts";

const unchangedSession = async () => {
  // Draft choices apply when the session starts.
};
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
  const {
    reloadNewChatDraft: reload,
    discardNewChatDraft: discard,
    updateNewChatDraft: update,
    selectDraftHarness: chooseHarness,
    selectDraftModel: chooseModel,
  } = useWorkbenchStore.getState();
  const composerRef = useRef<HTMLTextAreaElement>(null);
  const surfaceRef = useRef<HTMLElement>(null);
  const submission = draft?.submission;
  const draftHarnessId = draft?.harnessId;
  const released = submission?.status === "released";
  const submitting = submission?.status === "submitting";
  const ownsChat = useWorkbenchStore((state) => !!draftOwnedChatId(state));
  const unconfirmed =
    submission?.status === "unconfirmed" ||
    (submitting && submission.creation.status === "unconfirmed");
  useComposerFocusHotkey(composerRef, !submitting && !transitioning);
  const harness = harnesses.find((harness) => harness.id === draftHarnessId);
  useEffect(() => {
    (composerRef.current?.disabled
      ? surfaceRef.current
      : (composerRef.current ?? surfaceRef.current)
    )?.focus();
  }, [focusRequest]);
  useEffect(() => {
    if (transitioning) return;
    const harnessId =
      (released
        ? preferredAvailableHarness(harnesses, draftHarnessId)?.id
        : null) ?? draftHarnessId;
    if (
      harnessId &&
      (harnessId !== draftHarnessId || selectedHarnessId !== harnessId)
    ) {
      void chooseHarness(harnessId);
    }
  }, [
    chooseHarness,
    draftHarnessId,
    released,
    harnesses,
    selectedHarnessId,
    transitioning,
  ]);
  if (!draft) return null;
  const targetId =
    draft.targetProjectId === undefined
      ? draft.projectId
      : draft.targetProjectId;
  // Only its name is shown.
  const shownProject = projects.find((item) => item.id === targetId);
  const available = harness?.installState === "available";
  const bindingId = harness?.bindingId;
  const choice = effectiveDraftChoice(draft, {
    harnesses,
    selectedHarnessId,
    harnessDiscoveryPhase: discoveryPhase,
    models,
  });
  const errorNotice = unconfirmed ? (
    <>
      {draft.error ??
        "Chat creation is unconfirmed. Reload chats before trying again."}
      <button
        type="button"
        className="button button--compact"
        disabled={transitioning || !!pendingFirstSend}
        onClick={reload}
      >
        Reload chats
      </button>
    </>
  ) : (
    draft.error
  );
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
        {!choice && !errorNotice ? (
          <LazyHarnessNotice harness={harness ?? null} compact />
        ) : null}
        <ChatComposerRegion
          composerRef={composerRef}
          fieldId="new-chat-prompt"
          dictationScope=""
          descriptionId="new-chat-description"
          disabledReasonId="new-chat-disabled"
          text={draft.text}
          attachedContext={draft.images ?? []}
          // Images go to a chat's native session, so only with a binding.
          imageInputAvailable={!!bindingId}
          onAddImages={
            bindingId
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
          accessTarget={{ bindingId }}
          draftPickers={{
            access: (choice ?? draft).access,
            effort: (choice ?? draft).effort,
            onAccessChange: (access) => update({ access }),
            onEffortChange: (effort) =>
              update({ effort: effort === "default" ? null : effort }),
          }}
          availableBinding={!!bindingId}
          canPrompt={available}
          discoveryPhase={discoveryPhase}
          selectedHarnessId={draftHarnessId ?? null}
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
