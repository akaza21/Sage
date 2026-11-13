import { useState } from "react";
import { useWorkbenchStore } from "../../app/store";
import { errorMessage } from "../../domain/errors";
import { SettingsRow } from "./SettingsRow";

export function AgentArchivedSettings() {
  const project = useWorkbenchStore((state) => state.project);
  const projects = useWorkbenchStore((state) => state.projects);
  const archived = useWorkbenchStore((state) => state.archivedChats);
  const transitioning = useWorkbenchStore(
    (state) => state.projectTransitioning,
  );
  const restore = useWorkbenchStore((state) => state.restoreChat);
  const [chosenProject, setChosenProject] = useState<string | null>(null);
  const [offset, setOffset] = useState(0);
  const [pending, setPending] = useState(false);
  const [message, setMessage] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const projectId = chosenProject ?? project?.id;
  const chats =
    projectId === project?.id
      ? archived
      : (projects.find((item) => item.id === projectId)?.railArchived ?? []);
  const start = Math.min(
    offset,
    Math.max(0, Math.floor((chats.length - 1) / 50) * 50),
  );
  async function unarchive(chatId: string) {
    if (!projectId || pending) return;
    setPending(true);
    setError(null);
    setMessage(null);
    try {
      if (await restore(chatId, projectId)) setMessage("Chat unarchived.");
      else
        setError(
          useWorkbenchStore.getState().error ?? "Chat could not be unarchived.",
        );
    } catch (cause) {
      setError(errorMessage(cause));
    } finally {
      setPending(false);
    }
  }
  return (
    <>
      <p>
        Restore archived chats to their project without deleting conversation
        history.
      </p>
      <div className="agent-settings__card">
        <SettingsRow title="Project">
          <select
            className="select"
            aria-label="Archived chats project"
            value={projectId ?? ""}
            disabled={pending || transitioning}
            onChange={(event) => {
              setChosenProject(event.target.value);
              setOffset(0);
              setMessage(null);
              setError(null);
            }}
          >
            {project ? (
              <option value={project.id}>{project.name}</option>
            ) : (
              <option value="">No project</option>
            )}
            {projects
              .slice(0, 200)
              .filter((item) => item.id !== project?.id)
              .map((item) => (
                <option value={item.id} key={item.id}>
                  {item.name}
                </option>
              ))}
          </select>
        </SettingsRow>
      </div>
      {chats.length ? (
        <div className="agent-settings__card">
          {chats.slice(start, start + 50).map((chat) => (
            <SettingsRow
              key={chat.id}
              title={chat.title}
              description="Archived chat"
            >
              <button
                className="button button--compact"
                type="button"
                aria-label={`Unarchive ${chat.title}`}
                disabled={pending || transitioning || !project}
                onClick={() => void unarchive(chat.id)}
              >
                Unarchive
              </button>
            </SettingsRow>
          ))}
        </div>
      ) : null}
      {!chats.length ? (
        <p role="status">No archived chats in this project.</p>
      ) : null}
      {chats.length > 50 ? (
        <div className="agent-settings__pagination">
          <button
            type="button"
            className="button button--compact"
            disabled={start === 0 || pending}
            onClick={() => setOffset(start - 50)}
          >
            Previous chats
          </button>
          <span>
            {start + 1}–{Math.min(start + 50, chats.length)} of {chats.length}
          </span>
          <button
            type="button"
            className="button button--compact"
            disabled={start + 50 >= chats.length || pending}
            onClick={() => setOffset(start + 50)}
          >
            Next chats
          </button>
        </div>
      ) : null}
      {message ? <p role="status">{message}</p> : null}
      {error ? <p role="alert">{error}</p> : null}
    </>
  );
}
