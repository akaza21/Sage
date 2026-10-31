import { SageMark } from "../../ui/leanIconCore";
import { ProjectScopeChip } from "./ProjectScopeChip";
import "./chat-home.css";
import "./chat-transcript-polish.css";

export function ChatHome({ projectName }: { projectName: string | null }) {
  return (
    <section className="chat-home" aria-label="New chat">
      <SageMark />
      <h3
        aria-label={
          projectName ? `What should we work on in ${projectName}?` : undefined
        }
      >
        What should we work on
        {projectName ? (
          <>
            {" "}
            in <ProjectScopeChip projectName={projectName} />
          </>
        ) : null}
        ?
      </h3>
    </section>
  );
}
