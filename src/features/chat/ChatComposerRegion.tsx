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
import { ComposerStatusNotices } from "./ComposerStatusNotices";
import { ComposerTurnControl } from "./ComposerTurnControl";
import { useComposerFiles } from "./useComposerFiles";
import { AttachmentsPart, GoalPart, QueuedRows, lazyParts } from "./lazyParts";
import { TranscriptPart } from "./transcriptLazy";
import { useBoundGoalDetails } from "./useBoundGoalDetails";
import { useComposerImageInput } from "./useComposerImageInput";
import { ComposerPasteSlot } from "./ComposerPastePart";
import { useComposerPaste } from "./useComposerPaste";
import type * as SlashChunk from "./ComposerSlashController";

// The `/` command menu loads with the field's first focus.
const SlashPart = lazyParts<typeof SlashChunk>(
  () => import("./ComposerSlashController"),
);

export function ChatComposerRegion(props: ChatComposerRegionProps) {
  const {
    hidden = false,
    approvalSlot,
    onEditSummaryMount,
    composerRef,
    fieldId,
    dictationScope,
    commandChatId,
    sessionId = null,
    project,
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
    sendBlocked = false,
    actionBusy,
    activeTurn,
    queueAvailable,
    queuedPrompts,
    steerAvailable,
    projectTransitioning,
    workbenchError,
    chatError,
    signInHarness,
    claudeSignInRequired,
    claudeAuthScope,
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
    onSubmit,
    onTextChange,
    onRemoveContext,
    imageInputAvailable = false,
    imageInputDisabledReason = null,
    onAddImages,
    onCancelTurn,
    onDeleteQueuedPrompt,
    onRetryQueuedPrompt,
    onSteerQueuedPrompt,
    ...harnessControls
  } = props;
  const sessionFeatures = !harnessControls.draftPickers;
  const shortcutLabel = composerShortcutLabel(/mac/i.test(navigator.platform));
  const composerDisabled = !enabled || projectTransitioning;
  const goalDetails = useBoundGoalDetails(
    goal,
    dictationScope,
    availableBinding && sessionHarnessId === selectedHarnessId,
  );
  // Transcript quotes render in their own lazy chip row above the textarea.
  const otherContext = attachedContext.filter((c) => !isTranscriptQuote(c));
  const imageInput = useComposerImageInput({
    contexts: attachedContext,
    available: imageInputAvailable,
    unavailableReason: imageInputDisabledReason,
    disabled: composerDisabled,
    uploading: imageUploading,
    onAddImages,
  });
  // The paste part loads with the composer of a native chat, or with
  // held-back input, and reports the batch it staged: pasted text and files
  // picked with the attach button alike.
  const paste = useComposerPaste(
    sessionFeatures && availableBinding,
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
    (sessionFeatures && paste.blocksSend);
  const queueing = activeTurn && queueAvailable && !composerDisabled;
  const files = useComposerFiles({
    text,
    chatId: dictationScope,
    fieldRef: composerRef,
    fieldId,
    imageAction: imageInput.action,
    onPickFiles: paste.pickFiles,
    hidden: hidden || !sessionFeatures,
    disabled: composerDisabled,
    attachable: sessionFeatures && availableBinding,
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
    <div className="chat-dock surface surface--dock">
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
          if (dispatchBlocked) {
            event.preventDefault();
            return;
          }
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
              disabled={actionBusy || projectTransitioning}
              onRemove={onRemoveContext}
              chatId={dictationScope}
            />
          ) : null}
          <QueuedRows
            composerRef={composerRef}
            prompts={queuedPrompts}
            steerAvailable={steerAvailable}
            disabled={actionBusy || projectTransitioning}
            onDelete={onDeleteQueuedPrompt}
            onRetry={onRetryQueuedPrompt}
            onSteer={onSteerQueuedPrompt}
          />
        </div>
        <div className="composer__body">
          {goal.kind === "goal" ? (
            <GoalPart
              part="BoundChatGoalControl"
              state={goal}
              chatId={dictationScope}
              details={goalDetails}
            />
          ) : null}
          <ComposerStatusNotices
            workbenchError={workbenchError}
            chatError={chatError}
            signInHarness={signInHarness}
            claudeSignInRequired={claudeSignInRequired}
            claudeAuthScope={claudeAuthScope}
            imageStatus={imageInput.status}
            missingBindingNotice={missingBindingNotice}
            discoveryPhase={discoveryPhase}
          />
          <label className="sr-only" htmlFor={fieldId}>
            Message agent
          </label>
          <span className="sr-only" id={descriptionId}>
            {queueing ? "Queue" : "Send"} a message to the selected harness and
            model
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
              aria-description={
                queueing
                  ? "Enter queues a message while this turn runs."
                  : undefined
              }
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
                // Until the `/` menu loads, Enter holds a half-typed name.
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
          {sessionFeatures ? (
            <ComposerPasteSlot
              paste={paste}
              chatId={dictationScope}
              session={pasteSession}
              text={text}
              disabled={actionBusy || projectTransitioning}
            />
          ) : null}
          {disabledReason ? (
            <p className="composer__disabled-reason" id={disabledReasonId}>
              {disabledReason}
              {onRetryLoad ? (
                <button
                  type="button"
                  className="composer__disabled-retry"
                  onClick={onRetryLoad}
                >
                  Retry
                </button>
              ) : null}
            </p>
          ) : null}
          {otherContext.length > 0 ? (
            <div className="composer__attachment-row">
              <AttachmentsPart
                part="ComposerAttachments"
                contexts={otherContext}
                imagePreviews={imagePreviews}
                disabled={actionBusy || projectTransitioning}
                onRemove={onRemoveContext}
              />
            </div>
          ) : null}
          {sessionFeatures ? files.parts : null}
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
              availableBinding={availableBinding}
              canPrompt={canPrompt}
              sessionHarnessId={sessionHarnessId}
              selectedHarnessId={selectedHarnessId}
              modelLabelScope={JSON.stringify([dictationScope, sessionId])}
              inheritScope={dictationScope}
              projectName={composerWorkspaceName(project)}
              turnBusy={actionBusy || activeTurn}
              contextUsage={usage}
              onModeRemoved={() => composerRef.current?.focus()}
              attachSlot={sessionFeatures ? files.attachButton : null}
              addActions={{
                imageAction: imageInput.action,
                onOpenGoal: goalDetails.onOpen,
                onCompact,
              }}
            />
          ) : null}
          {!composerDisabled ? (
            <span className="composer__shortcut" aria-hidden="true">
              {shortcutLabel} to focus <span>/</span>{" "}
              {queueing ? "Enter to queue" : "Enter to send"}
            </span>
          ) : null}
          <ComposerTurnControl
            dictation={dictation}
            disabled={composerDisabled}
            onCancelTurn={onCancelTurn}
            cancelDisabled={actionBusy || projectTransitioning}
            queueing={Boolean(activeTurn && queueAvailable)}
            sendDisabled={
              dispatchBlocked ||
              (!text.trim() && !attachedContext.length && !paste.revision)
            }
          />
        </div>
      </form>
    </div>
  );
}
