import { MessageSquare } from "lucide-react";

export function ProjectChatRailGlyph({
  working,
  unread,
  attention,
}: {
  working: boolean;
  unread: boolean;
  attention: boolean;
}) {
  if (working) {
    return (
      <span className="project-chat__working" aria-label="Working">
        <span />
        <span />
        <span />
      </span>
    );
  }
  if (unread || attention) {
    return (
      <span
        className={`agent-unread-dot${
          attention ? " agent-unread-dot--attention" : ""
        }`}
        aria-label={unread ? "Unread updates" : "Needs attention"}
      />
    );
  }
  return <MessageSquare size={14} aria-hidden="true" />;
}
