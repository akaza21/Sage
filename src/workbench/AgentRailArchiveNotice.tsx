import { useEffect, useState } from "react";
import { useWorkbenchStore } from "../app/store";
import {
  setAgentRailArchiveUndo,
  useAgentRailInteractions,
} from "./agentRailInteractions";

export function AgentRailArchiveNotice() {
  const { undo } = useAgentRailInteractions();
  const transitioning = useWorkbenchStore(
    (state) => state.projectTransitioning,
  );
  const restoreChat = useWorkbenchStore((state) => state.restoreChat);
  const [saving, setSaving] = useState(false);
  const archived = useWorkbenchStore(
    (state) =>
      !undo ||
      (undo.projectId === state.project?.id
        ? state.archivedChats.some((chat) => chat.id === undo.chatId)
        : state.projects
            .find((project) => project.id === undo.projectId)
            ?.railArchived?.some((chat) => chat.id === undo.chatId)),
  );
  useEffect(() => {
    if (undo && !archived && !transitioning)
      setAgentRailArchiveUndo(null, undo);
  }, [archived, transitioning, undo]);
  if (!undo) return null;
  return (
    <div role="status" className="project-empty-copy">
      Archived {undo.title}
      <button
        type="button"
        className="button"
        disabled={transitioning || saving}
        aria-label="Undo archive"
        onClick={() => {
          setSaving(true);
          void restoreChat(undo.chatId, undo.projectId)
            .then((committed) => {
              if (committed) setAgentRailArchiveUndo(null, undo);
            })
            .finally(() => setSaving(false));
        }}
      >
        Undo
      </button>
    </div>
  );
}
