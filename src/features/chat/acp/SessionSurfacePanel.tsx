import { useState } from "react";
import { decodeSessionSurface } from "../../../services/acpSessionSurface";
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
  const decoded = decodeSessionSurface(surface);
  const activities = decoded?.activities ?? [];
  const keepsFiles = decoded?.undoKeepsFiles === true;
  const undoTurn = decoded?.undoTurn;
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
          title={
            keepsFiles
              ? "Remove the last turn from the chat; files stay as they are"
              : "Remove the last turn from the chat and restore the files it changed"
          }
          onClick={() => {
            if (!confirming) return setConfirming(true);
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
