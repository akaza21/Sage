import type { useComposerDictation } from "./composerChrome";
import { ArrowUp, Square, Mic } from "../../ui/leanIcons";

export function ComposerTurnControl({
  dictation,
  activeTurn,
  hasContent,
  onCancelTurn,
  cancelDisabled,
  sendDisabled,
  disabledReason,
  disabledReasonId,
}: {
  dictation: ReturnType<typeof useComposerDictation>;
  activeTurn: boolean;
  hasContent: boolean;
  onCancelTurn: (() => void) | null;
  cancelDisabled: boolean;
  sendDisabled: boolean;
  disabledReason?: string | null;
  disabledReasonId?: string;
}) {
  const reason = (sendDisabled && disabledReason) || undefined;
  const microphone = dictation.listening || !hasContent;
  return (
    <>
      {activeTurn ? (
        <button
          key="cancel"
          type="button"
          className="send-button send-button--cancel"
          aria-label="Cancel turn"
          title="Voice input unavailable during a turn"
          disabled={cancelDisabled || !onCancelTurn}
          onClick={onCancelTurn ?? undefined}
        >
          <Square aria-hidden="true" />
        </button>
      ) : microphone ? (
        <button
          key="dictation"
          type="button"
          className="composer__mic"
          aria-label={
            dictation.listening
              ? "Stop dictation"
              : dictation.supported
                ? "Start dictation"
                : "Voice input unavailable"
          }
          title={
            dictation.supported
              ? "May use an online service"
              : "Voice input is unavailable in this WebView"
          }
          disabled={dictation.disabled}
          aria-pressed={dictation.listening}
          onMouseDown={(event) => event.preventDefault()}
          onClick={(event) => {
            event.preventDefault();
            if (dictation.listening) dictation.stop();
            else void dictation.start();
          }}
        >
          <Mic aria-hidden="true" />
        </button>
      ) : (
        // Remount so a press begun on Stop cannot activate Send on release.
        <button
          key="send"
          className="send-button"
          type="submit"
          aria-label="Send message"
          title={reason}
          aria-describedby={reason && disabledReasonId}
          disabled={sendDisabled}
        >
          <ArrowUp aria-hidden="true" />
        </button>
      )}
      {dictation.error ? (
        <span className="composer__dictation-error" role="alert">
          {dictation.error}
        </span>
      ) : null}
    </>
  );
}
