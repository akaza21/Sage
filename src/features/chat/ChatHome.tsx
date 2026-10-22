import { MessageCircleQuestion } from "../../ui/leanIcons";
import { ProjectScopeChip } from "./ProjectScopeChip";
import "./chat-home.css";

export function ChatHome({ projectName }: { projectName: string | null }) {
  return (
    <section className="chat-home" aria-label="New chat">
      <MessageCircleQuestion size={40} aria-hidden="true" />
      <h3
        aria-label={
          projectName ? `What should we work on in ${projectName}?` : undefined
        }
      >
        {projectName ? (
          <>
            What should we work on in{" "}
            <ProjectScopeChip projectName={projectName} />?
          </>
        ) : (
          "What should we work on?"
        )}
      </h3>
    </section>
  );
}
