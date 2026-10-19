import { acpProjectionStoreFor } from "../../../../src/app/acpProjectionRegistry";
import { useWorkbenchStore } from "../../../../src/app/store";
import type { AcpSessionMode } from "../../../../src/domain/acpSessionConfig";

export function installVisualComposerActionsFixture(
  modes: AcpSessionMode[],
  chatIndex = 0,
) {
  const chat = useWorkbenchStore
    .getState()
    .tabs.filter((tab) => tab.kind === "chat")[chatIndex];
  if (!chat) throw new Error("Add fixture requires a Chat");
  const projection = acpProjectionStoreFor(chat.id).getState();
  const session = projection.page?.sessions.find(
    (item) => item.id === projection.page?.currentSessionId,
  );
  if (!projection.page || !projection.scope || !session?.runtimeEpoch) {
    throw new Error("Add fixture requires an authoritative session");
  }
  projection.applyAuthoritativePage(projection.scope, {
    ...projection.page,
    goal: { kind: "unsupported" },
    sessionConfig: {
      sageAgentSessionId: session.id,
      runtimeEpoch: session.runtimeEpoch,
      configRevision: "1",
      configOptions: [],
      modes: modes.length
        ? { currentModeId: modes[0].id, availableModes: modes }
        : null,
    },
  });
}
