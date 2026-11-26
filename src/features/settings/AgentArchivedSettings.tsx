import { useState } from "react";
import { useWorkbenchStore } from "../../app/store";
import { errorMessage } from "../../domain/errors";
import { Notice } from "../../ui/Notice";
import { AgentDataSettings } from "./AgentDataSettings";
import { SettingsRow } from "./SettingsRow";

const PRIVATE = "scope:private";

export function AgentArchivedSettings() {
  const project = useWorkbenchStore((state) => state.project);
  const projects = useWorkbenchStore((state) => state.projects);
  const unscopedChats = useWorkbenchStore((state) => state.unscopedChats);
  const archived = useWorkbenchStore((state) => state.archivedChats);
  const transitioning = useWorkbenchStore(
    (state) => state.projectTransitioning,
  );
  const restore = useWorkbenchStore((state) => state.restoreChat);
  const [chosenProject, setChosenProject] = useState<string | null>(null);
  const [offset, setOffset] = useState(0);
  const [pending, setPending] = useState(false);
  const [message, setMessage] = useState<string | null>(null);
  const [error, setError] = useState<{
    chatId: string;
    message: string;
  } | null>(null);
  const projectId =
    chosenProject ?? (project?.isInternal ? PRIVATE : project?.id);
  const chats =
    projectId === PRIVATE
      ? unscopedChats
          .flatMap((group) =>
            (group.id === project?.id
              ? archived
              : (group.railArchived ?? [])
            ).map((chat) => ({ ...chat, projectId: group.id })),
          )
          .slice(0, 64)
      : (projectId === project?.id
          ? archived
          : (projects.find((item) => item.id === projectId)?.railArchived ?? [])
        ).map((chat) => ({ ...chat, projectId }));
  const start = Math.min(
    offset,
    Math.max(0, Math.floor((chats.length - 1) / 50) * 50),
  );
  async function unarchive(chatId: string, ownerId: string | undefined) {
    if (!ownerId || pending) return;
    setPending(true);
    setError(null);
    setMessage(null);
    try {
      if (await restore(chatId, ownerId)) setMessage("Chat unarchived.");
      else
        setError({
          chatId,
          message:
            useWorkbenchStore.getState().error ??
            "Chat could not be unarchived.",
        });
    } catch (cause) {
      setError({ chatId, message: errorMessage(cause) });
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
            {project && !project.isInternal ? (
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
            {unscopedChats.length ? (
              <option value={PRIVATE}>Chats without a project</option>
            ) : null}
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
              notice={
                error?.chatId === chat.id ? (
                  <Notice
                    identity={`settings:unarchive:${chat.id}`}
                    severity="warning"
                    message={error.message}
                  />
                ) : null
              }
            >
              <button
                className="button button--compact"
                type="button"
                aria-label={`Unarchive ${chat.title}`}
                disabled={pending || transitioning || !project}
                onClick={() => void unarchive(chat.id, chat.projectId)}
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
    </>
  );
}

/** The lazy entry for Settings > Archived chats and Settings > Data. */
export default function ArchivedOrDataPage({ page }: { page: string }) {
  return page === "Data" ? <AgentDataSettings /> : <AgentArchivedSettings />;
}
