import { useState } from "react";
import { decodeSessionSurface } from "../../../services/acpSessionSurface";
import { isSideChatId } from "../../../workbench/sideChatState";
import { FileRewindPreview, useFileRewindPreview } from "../FileRewindPreview";
import "./session-surface.css";
import { useSessionControl } from "./useSessionControl";

const STATE_LABEL = {
  running: "Running",
  completed: "Done",
  failed: "Failed",
  cancelled: "Stopped",
} as const;

// The protocol's activity kind for a harness process that outlives its turn.
// Hooks, retries and compaction show in their Turn instead.
const backgroundTasks = (surface: unknown) =>
  decodeSessionSurface(surface)?.activities.filter(
    (activity) => activity.kind === "backgroundTask",
  ) ?? [];

function SessionStatus({
  status,
}: {
  status: ReturnType<typeof useSessionControl>["status"];
}) {
  return status.kind === "done" || status.kind === "failed" ? (
    <p
      className="acp-session-surface__notice"
      data-level={status.kind === "failed" ? "error" : undefined}
      role="status"
    >
      {status.message}
    </p>
  ) : null;
}

/** The session's background processes, with Stop where the harness offers it. */
export function BackgroundProcesses({
  surface,
  chatId,
}: {
  surface: unknown;
  chatId: string;
}) {
  const { status, run } = useSessionControl(chatId || undefined);
  const tasks = backgroundTasks(surface);
  if (!tasks.length) return <p>No background processes reported.</p>;
  return (
    <>
      <ul className="acp-session-surface__activities">
        {tasks.map((activity) => (
          <li key={activity.key} data-state={activity.state}>
            <span className="acp-session-surface__label">{activity.label}</span>
            {activity.progress ? (
              <span className="acp-session-surface__progress">
                {activity.progress}
              </span>
            ) : null}
            <span className="acp-session-surface__state">
              {STATE_LABEL[activity.state]}
            </span>
            {activity.stoppable && activity.state === "running" ? (
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
      <SessionStatus status={status} />
    </>
  );
}

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
  const [listed, setListed] = useState(false);
  const decoded = decodeSessionSurface(surface);
  const tasks = backgroundTasks(surface);
  const keepsFiles = decoded?.undoKeepsFiles === true;
  const undoTurn = decoded?.undoTurn;
  // Only a session whose rewinds preview files is asked for one; any other
  // undoes as its harness offers.
  const previews = decoded?.undoPreviewsFiles;
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
  if (!tasks.length && !undoTurn) return null;
  const running = tasks.filter(({ state }) => state === "running").length;
  return (
    <section className="acp-session-surface" aria-label="Session activity">
      {tasks.length ? (
        <button
          type="button"
          className="acp-session-surface__summary"
          aria-expanded={listed}
          onClick={() => {
            // The task chat's header lists them under its resources; a side
            // chat has no such header, so its list opens here.
            const resources = isSideChatId(chatId)
              ? null
              : document.querySelector<HTMLButtonElement>(
                  ".agent-task-header__resources > button",
                );
            if (!resources) setListed((open) => !open);
            else if (resources.getAttribute("aria-expanded") !== "true")
              resources.click();
          }}
        >
          {`${tasks.length} background ${tasks.length === 1 ? "task" : "tasks"}${running ? ` · ${running} running` : ""}`}
        </button>
      ) : null}
      {listed && tasks.length ? (
        <BackgroundProcesses surface={surface} chatId={chatId} />
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
      <SessionStatus status={status} />
    </section>
  );
}
