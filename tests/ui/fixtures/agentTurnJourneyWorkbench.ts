import {
  acpProjectionStoreFor,
  clearAcpProjectionRegistry,
  pinAcpProjectionIds,
} from "../../../src/app/acpProjectionRegistry";
import { useAgentArtifactAdmissionStore } from "../../../src/app/agentArtifactAdmissionStore";
import { useAgentArtifactFollowStore } from "../../../src/app/agentArtifactFollowStore";
import { pinAgentChatOwner } from "../../../src/app/agentChatOwner";
import { useWorkbenchStore } from "../../../src/app/store";
import type { ProjectGeneration } from "../../../src/domain/acp";
import { clearUnreadAcks } from "../../../src/features/chat/acp/chatUnread";
import { resetChatReadMarks } from "../../../src/features/chat/acp/chatReadMarks";
import {
  initialPage,
  JOURNEY_IDS,
  MAX_COMPOSER_HISTORY,
  sessionId,
  type FixtureState,
} from "./agentTurnJourneyModel";
import { installJourneyServices } from "./agentTurnJourneyServices";
import { journeySnapshot } from "./agentTurnJourneySnapshot";

export async function createJourneyFixtureState(): Promise<FixtureState> {
  clearAcpProjectionRegistry();
  clearUnreadAcks();
  resetChatReadMarks();
  useAgentArtifactAdmissionStore.getState().clear();
  useAgentArtifactFollowStore.getState().clear();
  pinAgentChatOwner(null);

  const state = {
    eventCount: 0,
    eventHistory: [],
    pages: new Map([
      [JOURNEY_IDS.alpha, initialPage("alpha")],
      [JOURNEY_IDS.beta, initialPage("beta")],
      [JOURNEY_IDS.gamma, initialPage("gamma")],
    ]),
    sendRequests: [],
    cancelRequests: [],
    pendingSends: new Map(),
    pendingCancels: new Map(),
    listeners: new Set(),
    publications: [],
    composerHistory: [],
    activeProjectId: JOURNEY_IDS.projectA,
    activeChatByProject: new Map([
      [JOURNEY_IDS.projectA, JOURNEY_IDS.alpha],
      [JOURNEY_IDS.projectB, JOURNEY_IDS.gamma],
    ]),
    transitionEpoch: 7,
    disposeStoreSubscription: () => undefined,
    restore: () => undefined,
  } satisfies FixtureState;
  state.restore = installJourneyServices(state);

  const initial = journeySnapshot(
    state,
    JOURNEY_IDS.projectA,
    state.transitionEpoch,
  );
  pinAcpProjectionIds([JOURNEY_IDS.alpha, JOURNEY_IDS.beta]);
  for (const id of [JOURNEY_IDS.alpha, JOURNEY_IDS.beta]) {
    const scope = {
      projectId: JOURNEY_IDS.projectA,
      projectGeneration: "7" as ProjectGeneration,
      chatId: id,
    };
    const store = acpProjectionStoreFor(id);
    store.getState().activateScope(scope);
    store
      .getState()
      .applyAuthoritativePage(scope, structuredClone(state.pages.get(id)!));
  }
  const emptyChatUi = {
    composerText: "",
    attachedContext: [],
    sending: false,
    error: null,
    requestGeneration: 0,
  };
  useWorkbenchStore.setState({
    phase: "ready",
    error: null,
    projects: initial.projects,
    project: initial.project,
    tabs: initial.tabs,
    archivedChats: [],
    activeTabId: initial.activeTabId,
    selectedRootId: null,
    selectedResource: null,
    sidebarOpen: true,
    sidebarView: "explorer",
    projectGeneration: 7,
    nativeTransitionEpoch: 7,
    projectTransitioning: false,
    harnesses: initial.harnesses,
    selectedHarnessId: "journey-acp",
    models: [{ id: JOURNEY_IDS.model, name: "Journey model", isDefault: true }],
    selectedModelId: JOURNEY_IDS.model,
    harnessDiscoveryPhase: "ready",
    harnessDiscoveryError: null,
    sessionId: sessionId("alpha"),
    messages: [],
    chatOwnershipEpoch: 7,
    chatUiById: {
      [JOURNEY_IDS.alpha]: { ...emptyChatUi },
      [JOURNEY_IDS.beta]: { ...emptyChatUi },
    },
    composerText: "",
    sending: false,
    chatError: null,
    repositories: initial.repositories,
    turnChanges: [],
    directoryListings: {},
    expandedDirectories: {},
    diffs: {},
    activeEditor: null,
  });
  const previous = new Map<string, string>();
  state.disposeStoreSubscription = useWorkbenchStore.subscribe((workbench) => {
    for (const [id, entry] of Object.entries(workbench.chatUiById)) {
      if (previous.get(id) === entry.composerText) continue;
      previous.set(id, entry.composerText);
      if (state.composerHistory.length < MAX_COMPOSER_HISTORY) {
        state.composerHistory.push({ chatId: id, text: entry.composerText });
      }
    }
  });
  return state;
}
