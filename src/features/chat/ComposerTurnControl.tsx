import { ArrowUp, Square } from "../../ui/leanIcons";

export function ComposerTurnControl({
  onCancelTurn,
  cancelDisabled,
  sendDisabled,
  queueing,
}: {
  onCancelTurn: (() => void) | null;
  cancelDisabled: boolean;
  sendDisabled: boolean;
  queueing: boolean;
}) {
  return onCancelTurn ? (
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
  );
}
