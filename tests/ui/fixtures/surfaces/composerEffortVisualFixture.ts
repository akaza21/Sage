import { acpProjectionStoreFor } from "../../../../src/app/acpProjectionRegistry";
import { useWorkbenchStore } from "../../../../src/app/store";

export function installVisualComposerEffortFixture() {
  const chat = useWorkbenchStore
    .getState()
    .tabs.find((tab) => tab.kind === "chat");
  if (!chat) throw new Error("Thinking level fixture requires a Chat");
  const projection = acpProjectionStoreFor(chat.id);
  const current = projection.getState();
  const session = current.page?.sessions.find(
    (candidate) => candidate.id === current.page?.currentSessionId,
  );
  if (
    !current.page ||
    !current.scope ||
    !session ||
    session.runtimeEpoch === null
  ) {
    throw new Error("Thinking level fixture requires an authoritative session");
  }
  current.applyAuthoritativePage(current.scope, {
    ...current.page,
    sessionConfig: {
      sageAgentSessionId: session.id,
      runtimeEpoch: session.runtimeEpoch,
      configRevision: "1",
      modes: null,
      configOptions: [
        {
          id: "effort",
          name: "Thinking level",
          category: "effort",
          type: "select",
          currentValue: "medium",
          options: [
            { value: "low", name: "Low" },
            { value: "medium", name: "Medium" },
            { value: "high", name: "High" },
          ],
        },
      ],
    },
  });
}
