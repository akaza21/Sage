import { acpProjectionStoreFor } from "../../../src/app/acpProjectionRegistry";
import { useWorkbenchStore } from "../../../src/app/store";
import type { AcpItem } from "../../../src/domain/acp";

export function installPopoverSubagents(kind: "children" | "tasks") {
  const chat = useWorkbenchStore
    .getState()
    .tabs.find((tab) => tab.kind === "chat");
  if (!chat) throw new Error("Subagents fixture requires a Chat");
  const projection = acpProjectionStoreFor(chat.id).getState();
  if (!projection.page || !projection.scope)
    throw new Error("Subagents fixture requires a projection");
  const page = structuredClone(projection.page);
  const turn = page.turns.at(-1)!;
  const basis = turn.items[0];
  turn.activities = [];
  turn.items = [
    basis,
    ...["Reviewer", "Builder"].map(
      (label, index) =>
        ({
          ...basis,
          id: `popover-agent-${index}` as typeof basis.id,
          ordinal: index + 1,
          lifecycle: "completed",
          kind: kind === "children" ? "collaboration" : "toolCall",
          toolState:
            kind === "children" ? null : index ? "completed" : "inProgress",
          payload:
            kind === "children"
              ? {
                  activityId: null,
                  label: "spawnAgent",
                  summary: "",
                  receiverLabels: [label],
                  operation: "spawnAgent",
                  agentKeys: [`popover-agent-${index}`],
                  agentStates: [
                    [`popover-agent-${index}`, index ? "completed" : "running"],
                  ],
                }
              : {
                  presentation: {
                    kind: "subagent",
                    label,
                    activityId: null,
                    taskLifecycle: true,
                  },
                  inputSummary: null,
                },
        }) as AcpItem,
    ),
  ];
  page.sessions = page.sessions.map((session) => ({
    ...session,
    capabilities: {
      ...session.capabilities,
      childTranscriptRead: false,
      childConversationProjection: kind === "children",
      collaborationEventProjection: kind === "children",
    },
  }));
  // A newer revision, so a later reload of the scenario page cannot undo it.
  page.projectionRevision = String(
    BigInt(page.projectionRevision) + 1n,
  ) as typeof page.projectionRevision;
  projection.applyAuthoritativePage(projection.scope, page);
}
