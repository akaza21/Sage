import { acpProjectionStoreFor } from "../../../src/app/acpProjectionRegistry";
import { useWorkbenchStore } from "../../../src/app/store";
import { useSessionAccessModeStore } from "../../../src/app/sessionAccessModeStore";
import type {
  CuratedBindingId,
  CuratedModelId,
  SageItemId,
  TurnState,
} from "../../../src/domain/acp";
import { capabilitySetFixture } from "../../../src/domain/acpProjection.fixture";
import { acpDesktop } from "../../../src/services/acpDesktop";
import { popoverProfiles } from "./popoverProfiles";

export const chatChromePrompt =
  "Please inspect the chat chrome carefully.\nKeep every wrapped user prompt rectangular.\nCheck all supported harnesses at narrow widths.";
export function chatChromeAnswer(): string {
  return Array.from(
    { length: 60 },
    (_, index) =>
      `Paragraph ${index + 1}: A bounded transcript exercises scrolling and the latest-turn control.`,
  ).join("\n\n");
}

export function installChatChromeFixture(
  harnessId: string,
  home: boolean,
  turnState: TurnState = "completed",
  signIn = false,
) {
  const ids = ["codex", "claude-code", "pi", "opencode", "oh-my-pi"];
  const profile = popoverProfiles[ids.indexOf(harnessId)];
  if (!profile) throw new Error("Unknown chrome harness profile");
  const state = useWorkbenchStore.getState();
  const bindingId = `visual-${harnessId}` as CuratedBindingId;
  const modelId = `${harnessId}-model` as CuratedModelId;
  const modelName = `${profile.name} confirmed model`;
  const projectName = "Sage project with a long name";
  const title =
    "A long chat title that must truncate instead of wrapping the header";
  const chatUiById = { ...state.chatUiById };
  for (const tab of state.tabs.filter(
    (candidate) => candidate.kind === "chat",
  )) {
    chatUiById[tab.id] = {
      ...chatUiById[tab.id],
      harnessId,
      modelId,
    };
  }
  useSessionAccessModeStore.setState({ modes: {}, ready: true, error: null });
  useWorkbenchStore.setState({
    project: state.project ? { ...state.project, name: projectName } : null,
    projects: state.projects.map((project) => ({
      ...project,
      name: projectName,
    })),
    tabs: state.tabs.map((tab) => ({ ...tab, title })),
    archivedChats: [{ id: "chat:old", title: "Archived research" }],
    harnesses: [
      {
        id: harnessId,
        name: profile.name,
        description: "Capability-shaped chrome fixture",
        installState: "available",
        source: "builtIn",
        transport: "acp",
        bindingId,
        accountModelDiscovery: false,
        accessModes: profile.access,
      },
    ],
    selectedHarnessId: harnessId,
    selectedModelId: modelId,
    models: [{ id: modelId, name: modelName }],
    chatUiById,
    harnessDiscoveryPhase: signIn ? "signInRequired" : "ready",
  });
  // AP-A1: only displayed chats keep a pinned projection; the others load
  // their page when selected, so the fixture rewrites the settled ones.
  const settled = state.tabs.filter((tab) => {
    if (tab.kind !== "chat") return false;
    const projection = acpProjectionStoreFor(tab.id).getState();
    return Boolean(projection.page && projection.scope);
  });
  if (!settled.length)
    throw new Error("Chrome fixture needs a settled projection");
  const pages = settled.map((tab) => {
    const projection = acpProjectionStoreFor(tab.id).getState();
    if (!projection.page || !projection.scope)
      throw new Error("Chrome fixture needs a settled projection");
    const page = structuredClone(projection.page);
    page.title = title;
    page.sessions = page.sessions.map((session) => ({
      ...session,
      bindingId,
      // A live matching session reports Ready/Live by design; sign-in
      // discovery only surfaces when no session owns the selected model.
      modelId: signIn ? (`${modelId}-previous` as CuratedModelId) : modelId,
      state: "active" as const,
      capabilities: capabilitySetFixture({
        closeSession: false,
        configCount: profile.effort ? 1 : 0,
        childConversationProjection: profile.subagents === "children",
        collaborationEventProjection: profile.subagents === "children",
      }),
    }));
    const session = page.sessions.find(
      (candidate) => candidate.id === page.currentSessionId,
    )!;
    page.sessionConfig = {
      sageAgentSessionId: session.id,
      runtimeEpoch: session.runtimeEpoch!,
      configRevision: "1",
      confirmedModel: { id: modelId, name: modelName },
      modes: profile.modes.length
        ? {
            currentModeId: profile.modes[0].id,
            availableModes: profile.modes,
          }
        : null,
      configOptions: profile.effort
        ? [
            {
              id: profile.effort.id,
              category: "effort",
              name: "Thinking level",
              type: "select",
              currentValue: profile.effort.currentValue,
              options: profile.effort.levels.map((value) => ({
                value,
                name: value,
              })),
            },
          ]
        : [],
    };
    page.turns = home
      ? []
      : page.turns.slice(-1).map((turn) => {
          const basis = turn.items[0];
          return {
            ...turn,
            state: turnState,
            items: [
              {
                ...basis,
                id: `${turn.id}-user` as SageItemId,
                kind: "text" as const,
                toolState: null,
                lifecycle: "completed" as const,
                payload: {
                  role: "user" as const,
                  text: chatChromePrompt,
                },
              },
              {
                ...basis,
                id: `${turn.id}-answer` as SageItemId,
                kind: "text" as const,
                toolState: null,
                lifecycle: "completed" as const,
                payload: {
                  role: "assistant" as const,
                  text: chatChromeAnswer(),
                },
              },
            ],
          };
        });
    // Replacement content must wake revision-driven transcript effects before
    // a queued scroll event from the previous layout can pause following.
    page.projectionRevision = String(
      BigInt(page.projectionRevision) + 1n,
    ) as typeof page.projectionRevision;
    projection.applyAuthoritativePage(projection.scope, page);
    return page;
  });
  acpDesktop.getChatPage = async (request) => {
    const page = pages.find((candidate) => candidate.chatId === request.chatId);
    if (!page) throw new Error("Unknown fixture chat");
    return structuredClone(page);
  };
}
