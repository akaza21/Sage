import type { AgentArtifactOwner } from "../../../src/domain/agentArtifact";
import type { ChatHandoffIntent } from "../../../src/domain/chatHandoff";
import type { HarnessBrowseOwner } from "../../../src/features/preview/harnessBrowse";
import type {
  RootRepositoryStatus,
  WorkbenchTab,
} from "../../../src/domain/workbench";
import { diffCacheKey } from "../../../src/domain/diffIdentity";
import { resourceKey } from "../../../src/domain/resourceIdentity";
import { useWorkbenchStore } from "../../../src/app/store";
import { useAgentArtifactAdmissionStore } from "../../../src/app/agentArtifactAdmissionStore";
import {
  applyBrowseIntent,
  harnessBrowseOwner,
  setHarnessBrowseOwner,
  subscribeBrowseSessions,
} from "../../../src/features/preview/harnessBrowse";

import type { BuiltVisualScenario } from "./visualScenarioTypes";
import {
  BINDING_ID,
  FIXED_NOW,
  GENERATION,
  MODEL_ID,
  PROJECT_ID,
  RUNTIME_EPOCH,
  ROOT_ID,
  SESSION_ID,
} from "./visualFixtureIds";
import {
  editorContent,
  editorDocument,
  fileResource,
  visualDiff,
} from "./visualDocumentFixtures";

const VISUAL_NATIVE_TRANSITION_EPOCH = 0;
// Keep the fixture wait bounded while allowing StrictMode's lazy Chat import to
// settle under a fully concurrent Vitest run.
const PREVIEW_OWNER_DEADLINE_MS = 2_000;

function artifactOwner(
  chatId: AgentArtifactOwner["chatId"],
): AgentArtifactOwner {
  return { projectId: PROJECT_ID, chatId, sessionId: SESSION_ID };
}

function browseOwner(chatId: HarnessBrowseOwner["chatId"]): HarnessBrowseOwner {
  return {
    ...artifactOwner(chatId),
    projectGeneration: GENERATION,
    transitionEpoch: VISUAL_NATIVE_TRANSITION_EPOCH,
    runtimeEpoch: RUNTIME_EPOCH,
  };
}

function visualPreviewUrl(): string {
  const port = window.location.port || "1420";
  return `http://127.0.0.1:${port}/tests/ui/fixtures/preview.html`;
}

function sameBrowseOwner(
  left: HarnessBrowseOwner | null,
  right: HarnessBrowseOwner,
): boolean {
  return Boolean(
    left &&
    left.projectId === right.projectId &&
    left.projectGeneration === right.projectGeneration &&
    left.transitionEpoch === right.transitionEpoch &&
    left.chatId === right.chatId &&
    left.sessionId === right.sessionId &&
    left.runtimeEpoch === right.runtimeEpoch,
  );
}

export function applyVisualWorkbenchState(
  scenario: BuiltVisualScenario,
  tabs: WorkbenchTab[],
): void {
  const visualWindow = window as typeof window & {
    __activateVisualHandoffTarget?: () => void;
    __releaseVisualPickerHarnessReady?: () => void;
  };
  delete visualWindow.__activateVisualHandoffTarget;
  delete visualWindow.__releaseVisualPickerHarnessReady;
  delete document.documentElement.dataset.visualHarnessSelectionCount;
  delete document.documentElement.dataset.visualModelSelectionCount;
  delete document.documentElement.dataset.visualHandoffDispatchCount;
  if (scenario.name === "agent-picker-handoff") {
    document.documentElement.dataset.visualHandoffDispatchCount = "0";
    visualWindow.__activateVisualHandoffTarget = () => {
      delete visualWindow.__activateVisualHandoffTarget;
      useWorkbenchStore.setState({
        activeTabId: "visual-chat-handoff-target",
      });
    };
  }
  const chatIds = scenario.pages.map((pageValue) => pageValue.chatId);
  const activeTabId =
    scenario.resource === "file" ? "visual-file-tab" : (chatIds[0] ?? null);

  const editorDoc = editorDocument();
  const emptyOwnership = {
    composerText: "",
    sending: false,
    error: null,
    requestGeneration: 0,
  };
  const chatUiById = Object.fromEntries(
    chatIds.map((chatId) => [chatId, { ...emptyOwnership }]),
  );
  const pickerHarnessScenario = scenario.name === "agent-picker-harness-model";
  const handoffPickerScenario = scenario.name === "agent-picker-handoff";
  const handoffTabs = handoffPickerScenario
    ? [
        ...tabs,
        ...Array.from({ length: 62 }, (_, index) => ({
          id: `visual-handoff-target-${index + 2}`,
          kind: "chat" as const,
          title: `Indexed target ${index + 2}`,
          position: index + 2,
          pinned: false,
          dirty: false,
        })),
      ]
    : tabs;
  const freshModels = Array.from({ length: 128 }, (_, index) => ({
    id: `visual-fresh-model-${index + 1}`,
    name: `Fresh advertised model ${index + 1}`,
  }));
  const rootListing = {
    resource: { rootId: ROOT_ID, relativePath: "" },
    entries: [
      {
        name: "src",
        resource: { rootId: ROOT_ID, relativePath: "src" },
        kind: "directory" as const,
        byteLen: null,
      },
      {
        name: "README.md",
        resource: { rootId: ROOT_ID, relativePath: "README.md" },
        kind: "file" as const,
        byteLen: 2_048,
      },
    ],
    truncated: false,
  };
  const srcListing = {
    resource: { rootId: ROOT_ID, relativePath: "src" },
    entries: [
      {
        name: "workbench",
        resource: { rootId: ROOT_ID, relativePath: "src/workbench" },
        kind: "directory" as const,
        byteLen: null,
      },
    ],
    truncated: false,
  };
  const workbenchListing = {
    resource: { rootId: ROOT_ID, relativePath: "src/workbench" },
    entries: [
      {
        name: "AgentChatPanes.tsx",
        resource: fileResource,
        kind: "file" as const,
        byteLen: editorContent.length,
      },
    ],
    truncated: false,
  };
  const visualRootRepository: RootRepositoryStatus = {
    rootId: ROOT_ID,
    state: "repository",
    branch: "feature/editor-polish",
    upstream: null,
    ahead: 0,
    behind: 0,
    branches: ["feature/editor-polish", "main", "release/beta"],
    branchesTruncated: false,
    recentCommits: [
      { sha: "b7c12d66aa7de0f0a1", subject: "Polish editor tools" },
      { sha: "49a0ee42f7d08b93c2", subject: "Keep Git actions truthful" },
      {
        sha: "1e8c53a1c51f81f8aa",
        subject: "Align shared file surfaces",
      },
    ],
    recentCommitsTruncated: false,
    remotes: ["origin", "upstream"],
    remotesTruncated: false,
    changes: [
      {
        resource: fileResource,
        indexStatus: "unchanged",
        worktreeStatus: "modified",
        additions: 28,
        deletions: 11,
      },
    ],
    truncated: false,
    elapsedMs: 12,
    message: null,
  };

  const realHandoffChat = useWorkbenchStore.getState().handoffChat;
  useWorkbenchStore.setState({
    phase: "ready",
    error: null,
    projects: [
      {
        id: PROJECT_ID,
        name: "Sage",
        rootCount: 1,
        lastOpenedAt: FIXED_NOW,
        railChats: [
          ...scenario.pages.map((pageValue, index) => ({
            id: pageValue.chatId,
            title: pageValue.title,
            pinned: index === 0,
            position: index,
            status:
              scenario.name === "error"
                ? ("failed" as const)
                : scenario.name === "waiting"
                  ? ("waiting" as const)
                  : index === 1
                    ? ("working" as const)
                    : ("complete" as const),
            updatedAt: FIXED_NOW - index * 60_000,
          })),
          {
            id: "visual-chat-attention",
            title: "Keyboard walkthrough needs attention",
            pinned: false,
            position: 8,
            status: "waiting",
            updatedAt: FIXED_NOW - 5 * 60_000,
          },
        ],
        railChatsTruncated: false,
      },
    ],
    project: {
      id: PROJECT_ID,
      name: "Sage",
      roots: [
        {
          id: ROOT_ID,
          name: "Sage",
          ordinal: 0,
          availability: "available",
          message: null,
        },
      ],
    },
    projectGeneration: VISUAL_NATIVE_TRANSITION_EPOCH,
    nativeTransitionEpoch: VISUAL_NATIVE_TRANSITION_EPOCH,
    projectTransitioning: false,
    tabs: handoffTabs,
    archivedChats: [],
    activeTabId,
    selectedRootId: ROOT_ID,
    selectedResource: scenario.resource === "file" ? fileResource : null,
    sidebarOpen: true,
    sidebarView: "explorer",
    harnesses: [
      {
        id: "visual-acp",
        name: "Codex",
        description: "Deterministic ACP visual fixture",
        installState: "available",
        source: "development",
        accountModelDiscovery: false,
        transport: "acp",
        bindingId: BINDING_ID,
      },
      ...(pickerHarnessScenario
        ? [
            {
              id: "visual-unavailable",
              name: "Unavailable Codex",
              description: "Disabled picker fixture",
              installState: "unavailable" as const,
              source: "development" as const,
              accountModelDiscovery: false,
              transport: "acp" as const,
              bindingId: "visual-unavailable",
            },
          ]
        : []),
    ],
    selectedHarnessId: pickerHarnessScenario
      ? "visual-unavailable"
      : "visual-acp",
    models: pickerHarnessScenario
      ? [{ id: "visual-old-model", name: "Old advertised model" }]
      : [{ id: MODEL_ID, name: "GPT-5" }],
    selectedModelId: pickerHarnessScenario ? "visual-old-model" : MODEL_ID,
    harnessDiscoveryPhase: "ready",
    harnessDiscoveryError: null,
    sessionId: SESSION_ID,
    messages: [],
    chatUiById,
    chatError: null,
    composerText: "",
    sending: false,
    repositoryRefs: [{ rootId: ROOT_ID, relativePath: "" }],
    repositoryInventoryPhase: "ready",
    repositoryInventoryError: null,
    repositoryDiscoveryTruncated: false,
    repositoryInventoryProjectScope: `${PROJECT_ID}\0${VISUAL_NATIVE_TRANSITION_EPOCH}\0${VISUAL_NATIVE_TRANSITION_EPOCH}`,
    selectedRepository: { rootId: ROOT_ID, relativePath: "" },
    selectedRepositoryStatus: visualRootRepository,
    repositoryStatusPhase: "ready",
    repositoryStatusError: null,
    repositories: {
      roots: [visualRootRepository],
      truncated: false,
      elapsedMs: 12,
    },
    turnChanges: [],
    directoryListings: {
      [resourceKey(rootListing.resource)]: {
        phase: "ready",
        value: rootListing,
      },
      [resourceKey(srcListing.resource)]: {
        phase: "ready",
        value: srcListing,
      },
      [resourceKey(workbenchListing.resource)]: {
        phase: "ready",
        value: workbenchListing,
      },
    },
    expandedDirectories: {
      [resourceKey(srcListing.resource)]: true,
      [resourceKey(workbenchListing.resource)]: true,
    },
    diffs:
      scenario.resource === "diff"
        ? {}
        : {
            [diffCacheKey(undefined, fileResource)]: {
              phase: "ready",
              value: visualDiff,
            },
          },
    activeEditor:
      scenario.resource === "file"
        ? {
            phase: "ready",
            resource: fileResource,
            document: editorDoc,
            content: editorContent,
            revision: 0,
            persistedRevision: 0,
            persisting: false,
            saving: false,
            message: null,
          }
        : null,
    activateTab: async (tabId: string) => {
      useWorkbenchStore.setState({ activeTabId: tabId });
    },
    ...(handoffPickerScenario
      ? {
          handoffChat: async (intent: ChatHandoffIntent) => {
            const count = Number(
              document.documentElement.dataset.visualHandoffDispatchCount ??
                "0",
            );
            document.documentElement.dataset.visualHandoffDispatchCount =
              String(count + 1);
            return realHandoffChat(intent);
          },
        }
      : {}),
    ...(pickerHarnessScenario
      ? {
          selectHarness: async (harnessId: string) => {
            if (harnessId !== "visual-acp") return;
            const count = Number(
              document.documentElement.dataset.visualHarnessSelectionCount ??
                "0",
            );
            document.documentElement.dataset.visualHarnessSelectionCount =
              String(count + 1);
            useWorkbenchStore.setState({
              selectedHarnessId: harnessId,
              harnessDiscoveryPhase: "checking",
              models: [],
              selectedModelId: null,
            });
            visualWindow.__releaseVisualPickerHarnessReady = () => {
              delete visualWindow.__releaseVisualPickerHarnessReady;
              useWorkbenchStore.setState({
                harnessDiscoveryPhase: "ready",
                models: freshModels,
                selectedModelId: freshModels[0]?.id ?? null,
              });
            };
          },
          selectModel: (modelId: string) => {
            if (freshModels.some((model) => model.id === modelId)) {
              const count = Number(
                document.documentElement.dataset.visualModelSelectionCount ??
                  "0",
              );
              document.documentElement.dataset.visualModelSelectionCount =
                String(count + 1);
              useWorkbenchStore.setState({ selectedModelId: modelId });
            }
          },
        }
      : {}),
    loadRepositoryInventory: async () => undefined,
    loadDirectory: async () => undefined,
    // The diff fixture deliberately preserves the production loader so the
    // FileChange card reaches its exact immutable snapshot bridge.
    ...(scenario.resource === "diff"
      ? {}
      : { loadDiff: async () => undefined }),
  });

  const selectedArtifactOwner = chatIds[0] ? artifactOwner(chatIds[0]) : null;
  const selectedBrowseOwner = chatIds[0] ? browseOwner(chatIds[0]) : null;
  if (
    scenario.mode === "agent" &&
    scenario.resource === "file" &&
    selectedArtifactOwner
  ) {
    useAgentArtifactAdmissionStore.getState().admit({
      owner: selectedArtifactOwner,
      tabId: "visual-file-tab",
    });
  }
  if (scenario.resource !== "preview") {
    setHarnessBrowseOwner(selectedBrowseOwner, chatIds.slice(0, 2));
  }
}

function waitForMountedPreviewOwner(
  expectedOwner: HarnessBrowseOwner,
): Promise<void> {
  return new Promise((resolve, reject) => {
    let settled = false;
    let recheckFrame: number | null = null;
    let unsubscribe: () => void = () => undefined;
    const finish = (error?: Error) => {
      if (settled) return;
      settled = true;
      window.clearTimeout(deadline);
      if (recheckFrame !== null) cancelAnimationFrame(recheckFrame);
      unsubscribe();
      if (error) reject(error);
      else resolve();
    };
    const recheck = () => {
      if (!sameBrowseOwner(harnessBrowseOwner(), expectedOwner)) {
        if (recheckFrame !== null) cancelAnimationFrame(recheckFrame);
        recheckFrame = null;
        return;
      }
      if (recheckFrame !== null) return;
      recheckFrame = requestAnimationFrame(() => {
        recheckFrame = null;
        if (sameBrowseOwner(harnessBrowseOwner(), expectedOwner)) finish();
      });
    };
    const deadline = window.setTimeout(
      () =>
        finish(new Error("Visual Preview fixture owner was not established.")),
      PREVIEW_OWNER_DEADLINE_MS,
    );
    unsubscribe = subscribeBrowseSessions(recheck);
    recheck();
  });
}

export async function reapplyVisualPreviewBrowseIntent(
  scenario: BuiltVisualScenario,
): Promise<void> {
  if (scenario.resource !== "preview") return;
  const chatId = scenario.pages[0]?.chatId;
  if (!chatId) throw new Error("Visual Preview fixture requires a Chat.");
  const expectedOwner = browseOwner(chatId);
  await waitForMountedPreviewOwner(expectedOwner);
  if (!applyBrowseIntent({ owner: expectedOwner, url: visualPreviewUrl() })) {
    throw new Error("Visual Preview fixture browse intent was not admitted.");
  }
}
