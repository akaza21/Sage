import type {
  AcpChatPage,
  SageChatId,
  SageProjectId,
} from "../../../src/domain/acp";
import type {
  BootstrapSnapshot,
  ProjectListItem,
  SessionStatus,
  WorkbenchTab,
} from "../../../src/domain/workbench";
import { projectChatLiveSnapshot } from "../../../src/features/chat/acp/chatLiveSnapshot";
import {
  JOURNEY_IDS,
  sessionId,
  type FixtureState,
} from "./agentTurnJourneyModel";

const fixedNow = Date.UTC(2026, 8, 4, 12, 0, 0);

export function journeyRailStatus(
  page: AcpChatPage | undefined,
): SessionStatus {
  if (!page || page.turns.length === 0) return "idle";
  const snapshot = projectChatLiveSnapshot(page.chatId, page);
  return snapshot.statusTone === "idle" ? "complete" : snapshot.statusTone;
}

function projectRows(): ProjectListItem[] {
  return [
    {
      id: JOURNEY_IDS.projectA,
      name: "Project Alpha",
      rootCount: 0,
      lastOpenedAt: fixedNow,
      railChats: [
        {
          id: JOURNEY_IDS.alpha,
          title: "Alpha",
          pinned: true,
          position: 0,
          status: "complete",
        },
        {
          id: JOURNEY_IDS.beta,
          title: "Beta",
          pinned: false,
          position: 1,
          status: "complete",
        },
      ],
      railChatsTruncated: false,
    },
    {
      id: JOURNEY_IDS.projectB,
      name: "Project Beta",
      rootCount: 0,
      lastOpenedAt: fixedNow - 1_000,
      railChats: [
        {
          id: JOURNEY_IDS.gamma,
          title: "Gamma",
          pinned: false,
          position: 0,
          status: "complete",
        },
      ],
      railChatsTruncated: false,
    },
  ];
}

function tabsFor(projectId: SageProjectId): WorkbenchTab[] {
  const ids =
    projectId === JOURNEY_IDS.projectA
      ? [JOURNEY_IDS.alpha, JOURNEY_IDS.beta]
      : [JOURNEY_IDS.gamma];
  return ids.map((id, position) => ({
    id,
    kind: "chat" as const,
    title:
      id === JOURNEY_IDS.alpha
        ? "Alpha"
        : id === JOURNEY_IDS.beta
          ? "Beta"
          : "Gamma",
    position,
    pinned: position === 0,
    dirty: false,
    status: "complete" as const,
    updatedAt: fixedNow - position * 1_000,
  }));
}

export function journeySnapshot(
  state: FixtureState,
  projectId: SageProjectId,
  epoch: number,
): BootstrapSnapshot {
  const tabs = tabsFor(projectId).map((tab) => ({
    ...tab,
    status: journeyRailStatus(state.pages.get(tab.id as SageChatId)),
  }));
  return {
    transitionEpoch: epoch,
    projects: projectRows().map((project) => ({
      ...project,
      railChats: project.railChats?.map((chat) => ({
        ...chat,
        status: journeyRailStatus(state.pages.get(chat.id as SageChatId)),
      })),
    })),
    project: {
      id: projectId,
      name:
        projectId === JOURNEY_IDS.projectA ? "Project Alpha" : "Project Beta",
      roots: [],
    },
    tabs,
    archivedChats: [],
    activeTabId:
      state.activeChatByProject.get(projectId) ?? tabs[0]?.id ?? null,
    selectedRootId: null,
    selectedResource: null,
    harnesses: [
      {
        id: "journey-acp",
        name: "Journey ACP",
        description: "Bounded scripted ACP journey fixture",
        installState: "available",
        source: "development",
        accountModelDiscovery: false,
        transport: "acp",
        bindingId: JOURNEY_IDS.binding,
        // CP-29d: Send requires an advertised access mode.
        accessModes: [
          {
            id: "readOnly",
            label: "Read-only",
            description: "Read workspace files; no writes or network.",
            fallback: true,
          },
        ],
      },
    ],
    sessionId: sessionId(
      projectId === JOURNEY_IDS.projectA ? "alpha" : "gamma",
    ),
    selectedHarnessId: "journey-acp",
    selectedModelId: JOURNEY_IDS.model,
    messages: [],
    repositories: { roots: [], truncated: false, elapsedMs: 0 },
    turnChanges: [],
    recoveredBuffers: [],
    recoveryNotice: null,
  };
}
