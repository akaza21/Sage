import { LazyModuleBoundary } from "../../ui/LazyModuleBoundary";
const loadExecution = () =>
  Promise.all([
    import("./ChatQueueRuntime"),
    import("./ChatQueueExecution"),
  ]).then(([{ default: Runtime }, { default: Execution }]) => ({
    default: ({ chatId }: { chatId?: string }) => (
      <Runtime chatId={chatId} Execution={Execution} />
    ),
  }));

// Queue execution stays outside the sleeping Chat UI.
export function ChatQueueOwner({ chatId }: { chatId?: string }) {
  return (
    <LazyModuleBoundary
      label="Queued messages"
      load={loadExecution}
      fallback={null}
      render={(Execution) => <Execution chatId={chatId} />}
    />
  );
}
