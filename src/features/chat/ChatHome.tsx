import { MessageCircleQuestion } from "../../ui/leanIcons";
import { ProjectScopeChip } from "./ProjectScopeChip";
import "./chat-home.css";

export function ChatHome({ projectName }: { projectName: string | null }) {
  return (
    <section className="chat-home" aria-label="New chat">
      <MessageCircleQuestion size={40} aria-hidden="true" />
      <h3>
        {projectName
          ? `What should we work on in ${projectName}?`
          : "What should we work on?"}
      </h3>
      {projectName ? <ProjectScopeChip projectName={projectName} /> : null}
    </section>
  );
}
