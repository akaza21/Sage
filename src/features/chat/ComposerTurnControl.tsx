import type { useComposerDictation } from "./composerChrome";
import { ArrowUp, Square, Mic } from "../../ui/leanIcons";

export function ComposerTurnControl({
  dictation,
  disabled,
  onCancelTurn,
  cancelDisabled,
  sendDisabled,
  queueing,
}: {
  dictation: ReturnType<typeof useComposerDictation>;
  disabled: boolean;
  onCancelTurn: (() => void) | null;
  cancelDisabled: boolean;
  sendDisabled: boolean;
  queueing: boolean;
}) {
  return (
    <>
      <button
        className="composer__mic"
        type="button"
        aria-label={
          !dictation.supported
            ? "Voice input unavailable"
            : dictation.listening
              ? "Stop dictation"
              : "Start dictation"
        }
        title={
          dictation.supported
            ? "Speech may be processed by an online service"
            : "Voice input is unavailable in this WebView"
        }
        disabled={!dictation.supported || (disabled && !dictation.listening)}
        aria-pressed={dictation.listening}
        onMouseDown={(event) => event.preventDefault()}
        onClick={dictation.listening ? dictation.stop : dictation.start}
      >
        <Mic size={16} aria-hidden="true" />
      </button>
      {dictation.error ? (
        <span className="composer__dictation-error" role="alert">
          {dictation.error}
        </span>
      ) : null}
      {onCancelTurn ? (
        <button
          className="send-button send-button--cancel"
          type="button"
          aria-label="Cancel turn"
          disabled={cancelDisabled}
          onClick={onCancelTurn}
        >
          <Square size={14} fill="currentColor" aria-hidden="true" />
        </button>
      ) : (
        <button
          className="send-button"
          type="submit"
          aria-label={queueing ? "Queue message" : "Send message"}
          disabled={sendDisabled}
        >
          <ArrowUp size={20} aria-hidden="true" />
        </button>
      )}
    </>
  );
}
