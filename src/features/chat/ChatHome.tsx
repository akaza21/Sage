import { MessageCircleQuestion } from "../../ui/leanIcons";
import { ProjectScopeChip } from "./ProjectScopeChip";
import "./chat-home.css";

export function ChatHome({ projectName }: { projectName: string }) {
  return (
    <section className="chat-home" aria-label="New chat">
      <MessageCircleQuestion size={40} aria-hidden="true" />
      <h2 aria-label={`What should we work on in ${projectName}?`}>
        What should we work on in <ProjectScopeChip projectName={projectName} />
        ?
      </h2>
    </section>
  );
}
