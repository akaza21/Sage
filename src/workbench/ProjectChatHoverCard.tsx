import { createPortal } from "react-dom";
import { Folder } from "../ui/leanIcons";
import { agentRailHoverAge } from "./agentRailHoverAge";

const HOVER_CARD_WIDTH = 288;

// The owning row supplies the accessible text, so this hover card is
// aria-hidden.
export function ProjectChatHoverCard({
  row,
  shownAt,
  title,
  updatedAt,
  project,
}: {
  row: DOMRect;
  shownAt: number;
  title: string;
  updatedAt?: number;
  project?: string;
}) {
  const age = agentRailHoverAge(updatedAt, shownAt);
  const left = Math.max(
    8,
    Math.min(row.right, window.innerWidth - HOVER_CARD_WIDTH - 8),
  );
  return createPortal(
    <div
      className="agent-rail-hover-card"
      aria-hidden="true"
      style={{ left, top: row.top, width: HOVER_CARD_WIDTH }}
    >
      <div className="agent-rail-hover-card__head">
        <strong>{title}</strong>
        {age ? <span>{age}</span> : null}
      </div>
      {project ? (
        <div className="agent-rail-hover-card__project">
          <Folder size={12} />
          <span>{project}</span>
        </div>
      ) : null}
    </div>,
    document.body,
  );
}
