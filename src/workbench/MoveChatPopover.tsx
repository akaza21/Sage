import { Folder, FolderInput, Laptop } from "lucide-react";

import type { AgentChatMoveDestination } from "./agentChatRail";

export function MoveChatPopover({
  chatTitle,
  destinations,
  onMove,
  onClose,
}: {
  chatTitle: string;
  destinations: readonly AgentChatMoveDestination[];
  onMove: (projectId: string) => void;
  onClose: () => void;
}) {
  return (
    <div className="chat-move-popover" role="dialog" aria-label="Move chat">
      <div className="chat-move-popover__header">
        <strong>{chatTitle}</strong>
        <button type="button" aria-label="Close move chat" onClick={onClose}>
          Close
        </button>
      </div>
      {destinations.length === 0 ? (
        <p className="chat-move-popover__empty">No other project to move to.</p>
      ) : (
        <ul className="chat-move-popover__list">
          {destinations.map((destination) => (
            <li key={destination.id}>
              <button
                type="button"
                aria-label={`Move chat to ${destination.name}`}
                onClick={() => onMove(destination.id)}
              >
                {destination.kind === "local" ? (
                  <Laptop size={14} aria-hidden="true" />
                ) : (
                  <Folder size={14} aria-hidden="true" />
                )}
                <span>{destination.name}</span>
              </button>
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}

export function MoveChatButton({
  open,
  onToggle,
}: {
  open: boolean;
  onToggle: () => void;
}) {
  return (
    <button
      className="project-chat__move"
      type="button"
      aria-label="Move chat to another project"
      title="Move chat to another project"
      aria-expanded={open}
      onClick={onToggle}
    >
      <FolderInput size={12} aria-hidden="true" />
    </button>
  );
}
