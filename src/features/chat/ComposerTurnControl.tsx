import { useRef } from "react";
import type { useComposerDictation } from "./composerChrome";
import { ArrowUp, Square, Mic } from "../../ui/leanIcons";

export function ComposerTurnControl({
  dictation,
  activeTurn,
  hasContent,
  onCancelTurn,
  sendDisabled,
  disabledReason,
  disabledReasonId,
}: {
  dictation: ReturnType<typeof useComposerDictation>;
  activeTurn: boolean;
  hasContent: boolean;
  /** Absent while Cancel is unavailable. */
  onCancelTurn?: (() => void) | null;
  sendDisabled: boolean;
  disabledReason?: string | null;
  disabledReasonId?: string;
}) {
  // A press that began on Stop only ever stops, even if recording ended
  // on its own before the click landed. Key repeats keep the first press;
  // a press that ends without a click forgets it. Only pointerdown, Enter and
  // Space activate the button; any other key leaves the press unrecorded.
  const pressedStop = useRef(false);
  const notePress = (event: { type: string; key?: string; repeat?: boolean }) =>
    event.repeat ||
    (event.type === "keydown" && event.key !== " " && event.key !== "Enter") ||
    (pressedStop.current = dictation.listening);
  const forgetPress = () => (pressedStop.current = false);
  const reason = (sendDisabled && disabledReason) || undefined;
  return (
    <>
      <button
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
        onPointerDown={notePress}
        onKeyDown={notePress}
        onMouseLeave={forgetPress}
        onPointerCancel={forgetPress}
        onBlur={forgetPress}
        onMouseDown={(event) => event.preventDefault()}
        onClick={() => {
          if (pressedStop.current || dictation.listening) dictation.stop();
          else void dictation.start();
          forgetPress();
        }}
      >
        <Mic aria-hidden="true" />
      </button>
      {/* Muted until there is something to send; a running turn queues it. */}
      <button
        type="submit"
        className="send-button"
        aria-label="Send message"
        title={reason ?? (activeTurn ? "Queue message" : undefined)}
        aria-describedby={reason && disabledReasonId}
        data-ready={(hasContent && !sendDisabled) || undefined}
        disabled={sendDisabled || !hasContent}
        // Held Enter that stopped dictation must not repeat onto Send.
        onKeyDown={(event) =>
          event.repeat && event.key === "Enter" && event.preventDefault()
        }
      >
        <ArrowUp aria-hidden="true" />
      </button>
      {/* Stop sits beside Send while a turn runs, so typing still queues.
          When the turn ends, useTerminalTurnFocus returns focus to the
          prompt. */}
      {activeTurn ? (
        <button
          type="button"
          className="send-button send-button--cancel"
          aria-label="Cancel turn"
          title="Stop"
          disabled={!onCancelTurn}
          onClick={onCancelTurn ?? undefined}
        >
          <Square aria-hidden="true" />
        </button>
      ) : null}
      {dictation.error ? (
        <span className="composer__dictation-error" role="alert">
          {dictation.error}
        </span>
      ) : null}
    </>
  );
}
