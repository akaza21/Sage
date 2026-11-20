import { SageMark } from "../../ui/leanIconCore";
import { ProjectScopeChip } from "./LazyProjectScopeChip";
import "./chat-home.css";

export function ChatHome({ projectName }: { projectName: string | null }) {
  return (
    <section className="chat-home" aria-label="New chat">
      <SageMark />
      <h3>What should we work on?</h3>
      {projectName ? (
        <ProjectScopeChip projectName={projectName} variant="caption" />
      ) : null}
    </section>
  );
}
