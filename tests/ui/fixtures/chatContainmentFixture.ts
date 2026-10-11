import { acpProjectionStoreFor } from "../../../src/app/acpProjectionRegistry";
import { useWorkbenchStore } from "../../../src/app/store";
import { useSessionAccessModeStore } from "../../../src/app/sessionAccessModeStore";
import type { AcpChatPage } from "../../../src/domain/acp";

export interface ChatContainmentProfile {
  name: string;
  effort: boolean;
  context: boolean;
  goal: boolean;
  steer: boolean;
}

export function installChatContainmentFixture(
  profile: ChatContainmentProfile,
  live: boolean,
) {
  const store = useWorkbenchStore.getState();
  const harnesses = store.harnesses.map((harness) => ({
    ...harness,
    name: `${profile.name} ${"long-harness-".repeat(12)}`,
  }));
  const chatTabs = store.tabs.filter((tab) => tab.kind === "chat");
  const longTitle = "Review " + "long-resource-and-project-name-".repeat(12);
  useSessionAccessModeStore.setState({ ready: true });
  useWorkbenchStore.setState({
    harnesses,
    project: store.project
      ? {
          ...store.project,
          name: longTitle,
          roots: store.project.roots.map((root) => ({
            ...root,
            name: longTitle,
          })),
        }
      : null,
    tabs: store.tabs.map((tab) => ({ ...tab, title: longTitle })),
    models: [
      {
        id: store.selectedModelId!,
        name: "Advertised model " + "long-model-".repeat(20),
      },
    ],
    chatUiById: Object.fromEntries(
      chatTabs.map((tab) => [
        tab.id,
        {
          ...store.chatUiById[tab.id],
          composerText: "Draft survives resizing",
          queuedPrompts: live
            ? [
                {
                  id: "geometry-queue",
                  text: longTitle,
                  attachedContext: [
                    {
                      kind: "browserPage" as const,
                      tabId: "queued-page",
                      url: "https://example.test/",
                      title: longTitle,
                      text: "Bounded queued page",
                    },
                  ],
                  bindingId: harnesses[0].bindingId!,
                  modelId: store.selectedModelId,
                  status: "outcomeUnknown" as const,
                },
              ]
            : [],
        },
      ]),
    ),
  });
  for (const tab of chatTabs) {
    const projection = acpProjectionStoreFor(tab.id).getState();
    if (!projection.page || !projection.scope)
      throw new Error("Containment fixture requires a settled chat projection");
    const current = projection.page;
    const configOptions = profile.effort
      ? [
          {
            id: "effort",
            name: "Thinking level",
            category: "effort",
            type: "select" as const,
            currentValue: "high",
            options: [
              { value: "high", name: "High" },
              { value: "low", name: "Low" },
            ],
          },
        ]
      : [];
    const page: AcpChatPage = {
      ...current,
      title: longTitle,
      sessions: current.sessions.map((session) => ({
        ...session,
        state: live ? "active" : "ready",
        capabilities: {
          ...session.capabilities,
          activeTurnSteer: profile.steer,
        },
      })),
      sessionConfig: {
        sageAgentSessionId: current.currentSessionId!,
        runtimeEpoch: current.sessions[0].runtimeEpoch!,
        configRevision: "1",
        modes: null,
        configOptions,
      },
      goal:
        live && profile.goal
          ? {
              kind: "goal",
              snapshot: {
                objective: longTitle,
                status: "active",
                tokenBudget: 1000000,
                tokensUsed: 6000,
                timeUsedSeconds: 278125,
                createdAt: null,
                updatedAt: null,
              },
            }
          : { kind: "unsupported" },
      turns: live
        ? current.turns.map((turn, index) => ({
            ...turn,
            items: turn.items.map((item) =>
              item.kind === "fileChangeDiff"
                ? {
                    ...item,
                    payload: {
                      ...item.payload,
                      changes: item.payload.changes.map(
                        (change, changeIndex) =>
                          change.resource.kind === "resource"
                            ? {
                                ...change,
                                resource: {
                                  ...change.resource,
                                  resource: {
                                    ...change.resource.resource,
                                    relativePath: `src/${"long-file-name-".repeat(10)}${changeIndex}.tsx`,
                                  },
                                },
                              }
                            : change,
                      ),
                    },
                  }
                : item,
            ),
            state: index === current.turns.length - 1 ? "running" : turn.state,
            terminalAt:
              index === current.turns.length - 1 ? null : turn.terminalAt,
            usage: profile.context
              ? {
                  inputTokens: 6000,
                  outputTokens: 0,
                  totalTokens: 6000,
                  contextWindow: 272000,
                }
              : undefined,
          }))
        : [],
    };
    projection.applyAuthoritativePage(projection.scope, page);
  }
}
