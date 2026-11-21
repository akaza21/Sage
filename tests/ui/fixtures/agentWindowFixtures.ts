import { useWorkbenchStore } from "../../../src/app/store";
import { acpProjectionStoreFor } from "../../../src/app/acpProjectionRegistry";
import type {
  AcpChatPage,
  SageChatId,
  SageProjectId,
  ProjectionRevision,
} from "../../../src/domain/acp";
import type { BootstrapSnapshot } from "../../../src/domain/workbench";
import { getWorkbenchPorts } from "../../../src/ports/workbenchRuntime";
import { acpDesktop } from "../../../src/services/acpDesktop";
import { FIXED_NOW } from "./visualFixtureIds";

export function installUnsentChatFixture() {
  const state = useWorkbenchStore.getState();
  const projection = acpProjectionStoreFor(state.activeTabId!).getState();
  if (!projection.page || !projection.scope)
    throw new Error("Unsent fixture requires a settled Chat");
  if (projection.page.turns.length)
    throw new Error("Unsent fixture cannot discard existing turns");
  const page = {
    ...projection.page,
    projectionRevision: String(
      BigInt(projection.page.projectionRevision) + 1n,
    ) as ProjectionRevision,
    currentSessionId: null,
    sessions: [],
    sessionConfig: null,
  };
  projection.applyAuthoritativePage(projection.scope, page);
  const original = acpDesktop.getChatPage;
  acpDesktop.getChatPage = async (request, decodeGoal) =>
    request.chatId === page.chatId
      ? structuredClone(page)
      : original(request, decodeGoal);
  useWorkbenchStore.setState({ sessionId: null });
}

export function installPrivateRecentFixture() {
  useWorkbenchStore.setState({
    unscopedChats: [
      {
        id: "visual-private-recent",
        name: "",
        rootCount: 1,
        lastOpenedAt: FIXED_NOW,
        railChats: [
          {
            id: "visual-private-recent-chat",
            title: "Private work in progress",
            pinned: false,
            position: 0,
            status: "working",
            updatedAt: FIXED_NOW,
          },
        ],
      },
    ],
  });
}

export function installWindowNewChatFixture() {
  const original = acpDesktop.getChatPage;
  let created: AcpChatPage | null = null;
  acpDesktop.getChatPage = async (request, decodeGoal) =>
    created?.chatId === request.chatId
      ? structuredClone(created)
      : original(request, decodeGoal);
  getWorkbenchPorts().chat.createChat = async (
    layout,
    expected,
    projectId,
    chatId,
  ) => {
    const state = useWorkbenchStore.getState();
    if (!state.project) throw new Error("Window fixture needs a project");
    if (created) throw new Error("Window fixture creates only one Chat");
    if (!chatId) throw new Error("Window fixture needs the caller's Chat id");
    if (projectId && projectId !== state.project.id)
      throw new Error("Window fixture supports the current or private scope");
    const project =
      projectId === null
        ? {
            id: "visual-private-created",
            name: "",
            isInternal: true,
            roots: [
              {
                id: "visual-private-created-root",
                name: "Chat files",
                ordinal: 0,
                availability: "available" as const,
                message: null,
              },
            ],
          }
        : state.project;
    const id = chatId as SageChatId;
    created = {
      projectId: project.id as SageProjectId,
      chatId: id,
      title: "New chat",
      projectionRevision: "1" as ProjectionRevision,
      currentSessionId: null,
      sessions: [],
      turns: [],
      nextCursor: null,
    };
    const snapshot: BootstrapSnapshot = {
      transitionEpoch: expected + 1,
      projects: state.projects.map((item) =>
        item.id === layout?.projectId
          ? {
              ...item,
              railChats: layout.tabs
                .filter((tab) => tab.kind === "chat")
                .map((tab) => ({
                  ...item.railChats?.find((chat) => chat.id === tab.id),
                  id: tab.id,
                  title: tab.title,
                  pinned: tab.pinned,
                  position: tab.position,
                  updatedAt: tab.updatedAt ?? FIXED_NOW,
                })),
            }
          : item,
      ),
      unscopedChats: state.unscopedChats,
      project,
      tabs: [
        ...(projectId === null ? [] : (layout?.tabs ?? [])),
        {
          id,
          kind: "chat",
          title: "New chat",
          position: projectId === null ? 0 : (layout?.tabs.length ?? 0),
          pinned: false,
          dirty: false,
          updatedAt: FIXED_NOW,
        },
      ],
      activeTabId: id,
      selectedRootId: project.roots[0]?.id ?? null,
      selectedResource: projectId === null ? null : state.selectedResource,
      harnesses: state.harnesses,
      sessionId: null,
      selectedHarnessId: state.selectedHarnessId,
      selectedModelId: state.selectedModelId,
      messages: [],
      repositories:
        projectId === null
          ? { roots: [], truncated: false, elapsedMs: 0 }
          : state.repositories,
      turnChanges: [],
      recoveredBuffers: [],
      recoveryNotice: null,
      archivedChats: [],
    };
    return {
      committed: true,
      transitionEpoch: expected + 1,
      recoveryError: null,
      snapshot,
    };
  };
}
