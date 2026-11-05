import { SageMark } from "../../ui/leanIconCore";
import { ProjectScopeChip } from "./ProjectScopeChip";
import "./chat-home.css";
import "./chat-transcript-polish.css";

export function ChatHome({ projectName }: { projectName: string | null }) {
  return (
    <section className="chat-home" aria-label="New chat">
      <SageMark />
      <h3>What should we work on?</h3>
      {projectName ? <ProjectScopeChip projectName={projectName} /> : null}
    </section>
  );
}
