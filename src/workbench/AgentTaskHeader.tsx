import { Check, X } from "lucide-react";
import { useEffect, useRef, useState, type FormEvent } from "react";

import { useWorkbenchStore } from "../app/store";
import { MAX_CHAT_TITLE_BYTES, chatDisplayTitle } from "../domain/chatTitle";
import type { WorkbenchTab } from "../domain/workbench";
import type { ChatLiveSnapshot } from "../features/chat/acp/chatLiveSnapshot";
import { agentChatMoveDestinations } from "./agentChatRail";
import { ProjectChatActionMenu } from "./ProjectChatActionMenu";

export function AgentTaskHeader({
  tab,
  chatCount,
  snapshot,
}: {
  tab: WorkbenchTab | null;
  chatCount: number;
  snapshot: ChatLiveSnapshot | null;
}) {
  const project = useWorkbenchStore((state) => state.project);
  const projects = useWorkbenchStore((state) => state.projects);
  const renameChat = useWorkbenchStore((state) => state.renameChat);
  const archiveChat = useWorkbenchStore((state) => state.archiveChat);
  const pinChat = useWorkbenchStore((state) => state.pinChat);
  const moveChat = useWorkbenchStore((state) => state.moveChat);
  const projectTransitioning = useWorkbenchStore(
    (state) => state.projectTransitioning,
  );
  const [menuOpen, setMenuOpen] = useState(false);
  const [renaming, setRenaming] = useState(false);
  const [draft, setDraft] = useState("");
  const [renameError, setRenameError] = useState<string | null>(null);
  const inputRef = useRef<HTMLInputElement | null>(null);

  useEffect(() => {
    if (renaming) inputRef.current?.focus();
  }, [renaming]);

  if (!tab || !project) return null;
  const currentTab = tab;
  const currentProject = project;
  const title = chatDisplayTitle(currentTab.title);
  const destinations = agentChatMoveDestinations(projects, currentProject.id);

  function beginRename() {
    setDraft(title);
    setRenameError(null);
    setRenaming(true);
  }

  async function submitRename(event: FormEvent) {
    event.preventDefault();
    setRenameError(null);
    const committed = await renameChat(currentTab.id, draft, currentProject.id);
    if (committed) {
      setRenaming(false);
      return;
    }
    setRenameError(
      useWorkbenchStore.getState().error ?? "Chat rename could not be saved",
    );
  }

  return (
    <header className="agent-task-header">
      <div className="agent-task-header__breadcrumb" aria-label="Chat location">
        <span className="agent-task-header__project">
          {currentProject.name}
        </span>
        <span className="agent-task-header__separator" aria-hidden="true">
          /
        </span>
        {renaming ? (
          <form className="agent-task-header__rename" onSubmit={submitRename}>
            <input
              ref={inputRef}
              aria-label="New chat name"
              aria-invalid={renameError ? true : undefined}
              maxLength={MAX_CHAT_TITLE_BYTES}
              value={draft}
              disabled={projectTransitioning}
              onChange={(event) => setDraft(event.currentTarget.value)}
              onKeyDown={(event) => {
                if (event.key === "Escape") {
                  event.preventDefault();
                  setRenaming(false);
                  setRenameError(null);
                }
              }}
            />
            <button
              type="submit"
              aria-label="Save chat name"
              disabled={projectTransitioning}
            >
              <Check size={13} aria-hidden="true" />
            </button>
            <button
              type="button"
              aria-label="Cancel chat rename"
              disabled={projectTransitioning}
              onClick={() => {
                setRenaming(false);
                setRenameError(null);
              }}
            >
              <X size={13} aria-hidden="true" />
            </button>
            {renameError ? <span role="alert">{renameError}</span> : null}
          </form>
        ) : (
          <strong className="agent-task-header__title" title={title}>
            {title}
          </strong>
        )}
      </div>
      <span
        className={`agent-task-header__status agent-chat-chip agent-chat-chip--${snapshot?.statusTone ?? "idle"}`}
        role="status"
        aria-label={`Chat status: ${snapshot?.statusLabel ?? "Idle"}`}
      >
        {snapshot?.statusLabel ?? "Idle"}
      </span>
      <ProjectChatActionMenu
        chatTitle={`${title} task`}
        pinned={currentTab.pinned}
        open={menuOpen}
        disabled={projectTransitioning || renaming}
        archiveDisabled={projectTransitioning || chatCount <= 1}
        destinations={destinations}
        onOpenChange={setMenuOpen}
        onPin={() => void pinChat(currentTab.id, currentProject.id)}
        onRename={beginRename}
        onMove={(targetProjectId) =>
          void moveChat(currentTab.id, targetProjectId, currentProject.id)
        }
        onArchive={() => void archiveChat(currentTab.id, currentProject.id)}
        triggerRef={() => undefined}
      />
    </header>
  );
}
