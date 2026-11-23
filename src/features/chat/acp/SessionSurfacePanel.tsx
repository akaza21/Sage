import { useState } from "react";
import { decodeSessionSurface } from "../../../services/acpSessionSurface";
import { FileRewindPreview, useFileRewindPreview } from "../FileRewindPreview";
import "./session-surface.css";
import { currentContext } from "./useAcpChatActions";
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
  // Only a session whose rewinds preview files is asked for one; any other
  // undoes as its harness offers.
  const page = currentContext(chatId)?.page;
  const previews = !!page?.sessions.find(
    (session) => session.id === page.currentSessionId,
  )?.capabilities.rewind?.effects.filePreview;
  const found = useFileRewindPreview(
    chatId,
    confirming && previews ? undoTurn : null,
    asked,
  );
  // Files restore only once the preview lists nothing they would keep; a
  // failed check or a changed file leaves the conversation-only undo.
  const held =
    confirming &&
    previews &&
    !(typeof found === "object" && (!!found?.caveat || !found?.fileCount));
  const undo = (keepFiles?: true) => {
    setConfirming(false);
    void run(
      { kind: "undoTurn", turnId: undoTurn!, ...(keepFiles && { keepFiles }) },
      keepsFiles || keepFiles
        ? "Last turn removed; files unchanged"
        : "Changes undone",
    );
  };
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
        <div
          className="acp-session-surface__undo-group"
          onBlur={(event) => {
            if (!event.currentTarget.contains(event.relatedTarget))
              setConfirming(false);
          }}
        >
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
              if (!held) undo();
            }}
          >
            {confirming ? "Confirm undo" : "Undo last turn"}
          </button>
          {confirming ? (
            <div className="acp-session-surface__preview">
              <FileRewindPreview found={found} />
              {held && found !== undefined ? (
                <button
                  type="button"
                  className="acp-session-surface__undo"
                  onClick={() => undo(true)}
                >
                  Undo conversation only; files stay
                </button>
              ) : null}
            </div>
          ) : null}
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
