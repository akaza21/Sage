import { useState } from "react";
import { decodeSessionSurface } from "../../../services/acpSessionSurface";
import {
  FileRewindPreview,
  useFileRewindPreview,
} from "../FileRewindPreview";
import "./session-surface.css";
import { useSessionControl } from "./useSessionControl";

const STATE_LABEL = {
  running: "Running",
  completed: "Done",
  failed: "Failed",
  cancelled: "Stopped",
} as const;

export function SessionSurfacePanel({
  surface,
  chatId,
}: {
  surface: unknown;
  chatId: string;
}) {
  const { status, run } = useSessionControl(chatId || undefined);
  const [confirming, setConfirming] = useState(false);
  // Each confirm asks the native side again for what the undo would restore.
  const [asked, setAsked] = useState(0);
  const decoded = decodeSessionSurface(surface);
  const activities = decoded?.activities ?? [];
  const keepsFiles = decoded?.undoKeepsFiles === true;
  const undoTurn = decoded?.undoTurn;
  // The same dry run and conflict check as Rewind; `null` when the session
  // offers no file preview, and Undo then stays as it was.
  const found = useFileRewindPreview(
    chatId,
    confirming && !keepsFiles ? undoTurn : null,
    asked,
  );
  // Confirm waits for the preview and stays off when it failed.
  const held =
    confirming &&
    !keepsFiles &&
    (found === undefined || (!!found && "failed" in found));
  if (!activities.length && !undoTurn) return null;
  return (
    <section className="acp-session-surface" aria-label="Session activity">
      {activities.length ? (
        <ul className="acp-session-surface__activities">
          {activities.map((activity) => (
            <li key={activity.key} data-state={activity.state}>
              <span className="acp-session-surface__label">
                {activity.label}
              </span>
              {activity.progress ? (
                <span className="acp-session-surface__progress">
                  {activity.progress}
                </span>
              ) : null}
              <span className="acp-session-surface__state">
                {STATE_LABEL[activity.state]}
              </span>
              {activity.stoppable ? (
                <button
                  type="button"
                  disabled={status.kind === "pending"}
                  onClick={() =>
                    void run(
                      { kind: "stopActivity", activityKey: activity.key },
                      `Stopping ${activity.label}`,
                    )
                  }
                >
                  Stop
                </button>
              ) : null}
            </li>
          ))}
        </ul>
      ) : null}
      {undoTurn ? (
        <button
          type="button"
          className="acp-session-surface__undo"
          disabled={status.kind === "pending"}
          // Not `disabled`: that would drop focus and end the confirm.
          aria-disabled={held || undefined}
          title={
            keepsFiles
              ? "Remove the last turn from the chat; files stay as they are"
              : "Remove the last turn from the chat and restore the files it changed"
          }
          onClick={() => {
            if (!confirming) {
              setAsked((count) => count + 1);
              return setConfirming(true);
            }
            if (held) return;
            setConfirming(false);
            void run(
              { kind: "undoTurn", turnId: undoTurn },
              keepsFiles
                ? "Last turn removed; files unchanged"
                : "Changes undone",
            );
          }}
          onBlur={() => setConfirming(false)}
        >
          {confirming ? "Confirm undo" : "Undo last turn"}
        </button>
      ) : null}
      {undoTurn && confirming ? (
        <div className="acp-session-surface__preview">
          <FileRewindPreview found={found} />
        </div>
      ) : null}
      {status.kind === "done" || status.kind === "failed" ? (
        <p
          className="acp-session-surface__notice"
          data-level={status.kind === "failed" ? "error" : undefined}
          role="status"
        >
          {status.message}
        </p>
      ) : null}
    </section>
  );
}
