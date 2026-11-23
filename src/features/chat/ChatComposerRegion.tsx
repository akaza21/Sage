import { useRef, useState } from "react";
import type { ChatComposerRegionProps } from "./chatComposerRegionProps";
import { isImeComposing } from "../../ui/imeComposition";
import { keymapBinding, matchesKeymapChord } from "../../domain/keymapRegistry";
import { isTranscriptQuote } from "../../domain/transcriptQuote";
import "../../styles/queued-messages.css";
import { ChatHarnessControls } from "./ChatHarnessControls";
import {
  composerPromptPlaceholder,
  composerShortcutLabel,
  composerWorkspaceName,
  useComposerDictation,
} from "./composerChrome";
import { ComposerImageInput } from "./ComposerImageInput";
import type * as LocationChunk from "./ComposerLocationStrip";
const LocationPart = lazyParts<typeof LocationChunk>(
  () => import("./ComposerLocationStrip"),
);
import { ComposerStatusNotices } from "./ComposerStatusNotices";
import { composerProblem } from "./ComposerStatusNotices";
import { ComposerTurnControl } from "./ComposerTurnControl";
import { ComposerAvailabilityNotice } from "./ComposerAvailabilityNotice";
import { SIGN_IN_PHASES } from "./composerDisabledReason";
import { useComposerFiles } from "./useComposerFiles";
import { AttachmentsPart, GoalPart, QueuedRows, lazyParts } from "./lazyParts";
import { NoticePart, SlashPart } from "./composerLazyParts";
import { TranscriptPart } from "./transcriptLazy";
import { useBoundGoalDetails } from "./useBoundGoalDetails";
import { useComposerImageInput } from "./useComposerImageInput";
import { ComposerPasteSlot } from "./ComposerPastePart";
import { useComposerPaste } from "./useComposerPaste";

export function ChatComposerRegion(props: ChatComposerRegionProps) {
  const {
    hidden,
    approvalSlot,
    onEditSummaryMount,
    composerRef,
    fieldId,
    dictationScope,
    commandChatId,
    sessionId,
    project,
    newChat,
    usage,
    onCompact,
    descriptionId,
    disabledReasonId,
    text,
    attachedContext,
    pasteSession,
    imagePreviews,
    imageUploading = false,
    enabled,
    sendBlocked,
    actionBusy,
    activeTurn,
    queueAvailable,
    queuedPrompts,
    steerAvailable,
    projectTransitioning,
    workbenchError,
    chatError,
    chatAuth,
    availableBinding,
    canPrompt,
    discoveryPhase,
    sessionHarnessId,
    selectedHarnessId,
    sessionPromptUnavailable,
    missingBindingNotice,
    disabledReason,
    onRetryLoad,
    goal,
    goalCapabilities,
    onSubmit,
    onTextChange,
    onRemoveContext,
    imageInputAvailable = false,
    imageInputDisabledReason = null,
    onAddImages,
    onCancelTurn,
    onStop,
    onDeleteQueuedPrompt,
    onRetryQueuedPrompt,
    onSteerQueuedPrompt,
    ...harnessControls
  } = props;
  const sessionFeatures = !harnessControls.draftPickers;
  const shortcutLabel = composerShortcutLabel(/mac/i.test(navigator.platform));
  const composerDisabled = !enabled || projectTransitioning;
  const actionDisabled = actionBusy || projectTransitioning;
  const goalDetails = useBoundGoalDetails(
    availableBinding && sessionHarnessId === selectedHarnessId,
  );
  const otherContext = attachedContext.filter((c) => !isTranscriptQuote(c));
  const imageInput = useComposerImageInput({
    contexts: attachedContext,
    available: imageInputAvailable,
    unavailableReason: imageInputDisabledReason,
    disabled: composerDisabled,
    uploading: imageUploading,
    onAddImages,
  });
  const paste = useComposerPaste(
    availableBinding,
    activeTurn,
    disabledReasonId,
    onTextChange,
  );
  const dispatchBlocked =
    composerDisabled ||
    sendBlocked ||
    actionBusy ||
    (activeTurn && !queueAvailable) ||
    imageUploading ||
    imageInput.sendBlocked ||
    paste.blocksSend;
  const queueing = activeTurn && queueAvailable && !composerDisabled;
  const hasContent = Boolean(
    text.trim() || attachedContext.length || paste.revision,
  );
  const optionsTriggerId = `${fieldId}-options`;
  const projectName = composerWorkspaceName(project);
  const files = useComposerFiles({
    text,
    chatId: dictationScope,
    fieldRef: composerRef,
    fieldId,
    attachAnchorId: optionsTriggerId,
    imageAction: imageInput.action,
    onPickFiles: paste.pickFiles,
    hidden: hidden || !sessionFeatures,
    disabled: composerDisabled,
    attachable: availableBinding,
  });
  const shortcutDescriptionId = `${descriptionId}-shortcut`;
  const dictation = useComposerDictation(
    text,
    onTextChange,
    dictationScope,
    hidden || composerDisabled || activeTurn,
    composerRef,
  );
  const slashReady = useRef(false);
  const [slashArmed, setSlashArmed] = useState(false);
  return (
    <div className="chat-dock-frame">
      {newChat && !hidden ? (
        <LocationPart
          part="ComposerLocationStrip"
          projectName={projectName ?? "Choose project"}
          disabled={actionDisabled || !!sessionId}
          activeTurn={activeTurn}
        />
      ) : null}
      <div
        className="chat-dock surface surface--dock"
        data-working={(activeTurn && !hidden) || undefined}
      >
        <div className="chat-edit-summary-zone" ref={onEditSummaryMount} />
        {approvalSlot}
        <form
          className="composer"
          role="form"
          hidden={hidden}
          aria-busy={projectTransitioning || imageUploading}
          onDragOver={imageInput.onDragOver}
          onDrop={imageInput.onDrop}
          onPaste={imageInput.onPaste}
          onSubmit={(event) => {
            if (dispatchBlocked) return event.preventDefault();
            dictation.cancel();
            onSubmit(event, paste.revision);
            composerRef.current?.focus();
          }}
        >
          <div className="composer__slots">
            {otherContext.length < attachedContext.length ? (
              <TranscriptPart
                part="ComposerAnnotations"
                contexts={attachedContext}
                disabled={actionDisabled}
                onRemove={onRemoveContext}
                chatId={dictationScope}
              />
            ) : null}
            <QueuedRows
              composerRef={composerRef}
              prompts={queuedPrompts}
              steerAvailable={steerAvailable}
              disabled={actionDisabled}
              onDelete={onDeleteQueuedPrompt}
              onRetry={onRetryQueuedPrompt}
              onSteer={onSteerQueuedPrompt}
            />
          </div>
          <div className="composer__body">
            {goal?.kind === "goal" ||
            (goalCapabilities &&
              (!goalCapabilities.observes ||
                (goal?.kind === "noGoal" && goalCapabilities.set))) ? (
              <GoalPart
                part="BoundChatGoalControl"
                key={dictationScope}
                // Every chat passes its goal; only the new-chat draft has none.
                state={goal!}
                chatId={dictationScope}
                details={goalDetails}
                capabilities={goalCapabilities}
              />
            ) : null}
            <label className="sr-only" htmlFor={fieldId}>
              Message agent
            </label>
            <span className="sr-only" id={descriptionId}>
              {queueing ? "Queue" : "Send"} a message to the selected harness
              and model
            </span>
            {!composerDisabled ? (
              <span className="sr-only" id={shortcutDescriptionId}>
                {shortcutLabel} focuses the composer.{" "}
                {queueing
                  ? "Enter queues a message while this turn runs."
                  : "Enter sends."}{" "}
                Shift+Enter adds a new line.
              </span>
            ) : null}
            <div className="composer__input-area">
              {chatAuth?.page?.sessionSurface ? (
                <NoticePart part="ComposerChromeGate" {...props} />
              ) : null}
              <textarea
                ref={composerRef}
                id={fieldId}
                aria-label="Message agent"
                // A combobox for the `/` command menu (ARIA 1.2); its menu
                // controller owns aria-expanded once loaded.
                role="combobox"
                aria-expanded={false}
                spellCheck={false}
                autoCorrect="off"
                autoCapitalize="off"
                aria-describedby={`${descriptionId}${composerDisabled ? "" : ` ${shortcutDescriptionId}`}${disabledReason ? ` ${disabledReasonId}` : ""}${paste.describedBy}`}
                value={text}
                rows={2}
                placeholder={composerPromptPlaceholder(
                  canPrompt,
                  sessionPromptUnavailable,
                )}
                disabled={composerDisabled}
                {...(sessionFeatures
                  ? paste.field
                  : { onChange: (event) => onTextChange(event.target.value) })}
                onFocus={() => sessionFeatures && setSlashArmed(true)}
                onBlur={dictation.cancel}
                onKeyDown={(event) => {
                  // An IME composition owns its keys, Enter included.
                  if (isImeComposing(event)) return;
                  // While suggestions load, Enter must not send a half-typed
                  // command.
                  if (
                    sessionFeatures &&
                    !slashReady.current &&
                    event.key === "Enter" &&
                    !event.shiftKey &&
                    /^\/\S*$/u.test(text)
                  ) {
                    event.preventDefault();
                    return;
                  }
                  if (event.key === "Escape" && dictation.listening) {
                    event.preventDefault();
                    dictation.cancel();
                    return;
                  }
                  if (
                    enabled &&
                    matchesKeymapChord(keymapBinding("sendMessage"), event)
                  ) {
                    event.preventDefault();
                    event.currentTarget.form?.requestSubmit();
                  }
                }}
              />
              {sessionFeatures && slashArmed ? (
                <SlashPart
                  part="ComposerSlashController"
                  fieldRef={composerRef}
                  readyRef={slashReady}
                  boundChatId={commandChatId}
                  disabled={hidden || composerDisabled}
                  fieldId={fieldId}
                  text={text}
                  onTextChange={onTextChange}
                />
              ) : null}
            </div>
            <ComposerPasteSlot
              paste={paste}
              chatId={dictationScope}
              // The new-chat draft stages files in its own slot, a chat by id.
              draft={!sessionFeatures}
              session={pasteSession}
              text={text}
              disabled={actionDisabled}
            />
            <ComposerAvailabilityNotice
              reason={disabledReason}
              id={disabledReasonId}
              scope={dictationScope}
              onRetry={onRetryLoad}
              compact={
                // A visible error explains the session's own refusal; a local
                // reason (size, queue, access) stays visible.
                (!!chatError && sessionPromptUnavailable) ||
                missingBindingNotice ||
                (!availableBinding && !canPrompt) ||
                discoveryPhase === "checking" ||
                SIGN_IN_PHASES.includes(discoveryPhase)
              }
            />
            {otherContext.length > 0 ? (
              <div className="composer__attachment-row">
                <AttachmentsPart
                  part="ComposerAttachments"
                  contexts={otherContext}
                  imagePreviews={imagePreviews}
                  disabled={actionDisabled}
                  onRemove={onRemoveContext}
                />
              </div>
            ) : null}
            {files.parts}
            {!hidden ? (
              <ComposerStatusNotices
                workbenchError={workbenchError}
                chatError={chatError}
                chatAuth={chatAuth}
                imageStatus={composerProblem(
                  "image-input",
                  dictationScope,
                  imageInput.status,
                  "attachment",
                  "info",
                )}
              />
            ) : null}
          </div>
          <div className="composer__toolbar">
            {imageInputAvailable ? (
              <ComposerImageInput
                inputRef={imageInput.chooserRef}
                disabled={composerDisabled || imageUploading}
                onChoose={imageInput.accept}
              />
            ) : null}
            {!hidden ? (
              <ChatHarnessControls
                {...harnessControls}
                discoveryPhase={discoveryPhase}
                modelsPending={
                  discoveryPhase === "checking" || discoveryPhase === "idle"
                }
                availableBinding={availableBinding}
                canPrompt={canPrompt}
                sessionHarnessId={sessionHarnessId}
                selectedHarnessId={selectedHarnessId}
                modelLabelScope={JSON.stringify([dictationScope, sessionId])}
                inheritScope={dictationScope}
                projectName={projectName}
                projectReadOnly={!!sessionId}
                turnBusy={actionBusy || activeTurn}
                contextUsage={usage}
                onModeRemoved={() => composerRef.current?.focus()}
                addActions={{
                  boundChatId: commandChatId,
                  onAttachFiles: files.attach,
                  imageAction: imageInput.action,
                  attachRefusalChatId: files.refusalChatId,
                  ...goalDetails.actions,
                  onCompact,
                }}
                optionsTriggerId={optionsTriggerId}
              />
            ) : null}
            {!composerDisabled ? (
              <span className="composer__shortcut" aria-hidden="true">
                {shortcutLabel} to focus <span>/</span>{" "}
                {queueing ? "Enter to queue" : "Enter to send"}
              </span>
            ) : null}
            <ComposerTurnControl
              activeTurn={activeTurn}
              hasContent={hasContent}
              disabledReason={disabledReason}
              disabledReasonId={disabledReasonId}
              dictation={dictation}
              onCancelTurn={onStop ?? onCancelTurn}
              cancelDisabled={actionDisabled && !onStop}
              sendDisabled={dispatchBlocked}
            />
          </div>
        </form>
      </div>
    </div>
  );
}
