import { useWorkbenchStore } from "../../../src/app/store";
import type {
  AcpChatPage,
  SageChatId,
  SageProjectId,
  ProjectionRevision,
} from "../../../src/domain/acp";
import type { BootstrapSnapshot } from "../../../src/domain/workbench";
import { getWorkbenchPorts } from "../../../src/ports/workbenchRuntime";
import { acpDesktop } from "../../../src/services/acpDesktop";

export function installWindowNewChatFixture() {
  const original = acpDesktop.getChatPage;
  let created: AcpChatPage | null = null;
  acpDesktop.getChatPage = async (request, decodeGoal) =>
    created?.chatId === request.chatId
      ? structuredClone(created)
      : original(request, decodeGoal);
  getWorkbenchPorts().chat.createChat = async (layout, expected) => {
    const state = useWorkbenchStore.getState();
    if (!state.project) throw new Error("Window fixture needs a project");
    if (created) throw new Error("Window fixture creates only one Chat");
    const id = "ap-a-chat-1" as SageChatId;
    created = {
      projectId: state.project.id as SageProjectId,
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
      projects: state.projects,
      project: state.project,
      tabs: [
        ...layout.tabs,
        {
          id,
          kind: "chat",
          title: "New chat",
          position: layout.tabs.length,
          pinned: false,
          dirty: false,
        },
      ],
      activeTabId: id,
      selectedRootId: state.selectedRootId,
      selectedResource: state.selectedResource,
      harnesses: state.harnesses,
      sessionId: null,
      selectedHarnessId: state.selectedHarnessId,
      selectedModelId: state.selectedModelId,
      messages: [],
      repositories: state.repositories,
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
