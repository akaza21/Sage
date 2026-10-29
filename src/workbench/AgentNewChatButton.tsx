import { useWorkbenchStore } from "../app/store";
import { SquarePen } from "../ui/leanIcons";

export function AgentNewChatButton() {
  const transitioning = useWorkbenchStore(
    (state) => state.projectTransitioning,
  );
  return (
    <button
      className="agent-task-header__pencil"
      type="button"
      aria-label="New chat"
      title="New chat"
      disabled={transitioning}
      onClick={() => void useWorkbenchStore.getState().createChat()}
    >
      <SquarePen size={15} aria-hidden="true" />
    </button>
  );
}
