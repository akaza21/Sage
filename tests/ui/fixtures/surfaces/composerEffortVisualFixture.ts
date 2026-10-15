import { acpProjectionStoreFor } from "../../../../src/app/acpProjectionRegistry";
import { useWorkbenchStore } from "../../../../src/app/store";

export function installVisualComposerEffortFixture({
  id = "effort",
  levels = ["low", "medium", "high"],
  currentValue = "medium",
  defaultValue,
}: {
  id?: string;
  levels?: string[];
  currentValue?: string;
  defaultValue?: string;
} = {}) {
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
      confirmedModel: {
        id: session.modelId ?? "visual-model",
        name: "GPT-6 Astra",
      },
      modes: null,
      configOptions: [
        {
          id,
          name: "Thinking level",
          category: "effort",
          type: "select",
          currentValue,
          ...(defaultValue === undefined ? {} : { defaultValue }),
          options: levels.map((value) => ({
            value,
            name: value[0].toUpperCase() + value.slice(1),
          })),
        },
      ],
    },
  });
}
