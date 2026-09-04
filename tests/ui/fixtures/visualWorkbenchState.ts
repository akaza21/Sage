import type { AgentArtifactOwner } from "../../../src/domain/agentArtifact";
import type { WorkbenchTab } from "../../../src/domain/workbench";
import { diffCacheKey } from "../../../src/domain/diffIdentity";
import { resourceKey } from "../../../src/domain/resourceIdentity";
import { useWorkbenchStore } from "../../../src/app/store";
import { useAgentArtifactAdmissionStore } from "../../../src/app/agentArtifactAdmissionStore";
import {
  applyBrowseIntent,
  setHarnessBrowseOwner,
} from "../../../src/features/preview/harnessBrowse";

import type { BuiltVisualScenario } from "./visualScenarioTypes";
import {
  BINDING_ID,
  FIXED_NOW,
  MODEL_ID,
  PROJECT_ID,
  ROOT_ID,
  SESSION_ID,
} from "./visualFixtureIds";
import {
  editorContent,
  editorDocument,
  fileResource,
  visualDiff,
} from "./visualDocumentFixtures";

export function applyVisualWorkbenchState(
  scenario: BuiltVisualScenario,
  tabs: WorkbenchTab[],
): void {
  const chatIds = scenario.pages.map((pageValue) => pageValue.chatId);
  const activeTabId =
    scenario.resource === "file"
      ? "visual-file-tab"
      : scenario.resource === "diff"
        ? "visual-diff-tab"
        : (chatIds[0] ?? null);

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
    projectGeneration: 0,
    nativeTransitionEpoch: 0,
    projectTransitioning: false,
    tabs,
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
    ],
    selectedHarnessId: "visual-acp",
    models: [{ id: MODEL_ID, name: "GPT-5" }],
    selectedModelId: MODEL_ID,
    harnessDiscoveryPhase: "ready",
    harnessDiscoveryError: null,
    sessionId: SESSION_ID,
    messages: [],
    chatUiById,
    chatError: null,
    composerText: "",
    sending: false,
    repositories: {
      roots: [
        {
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
        },
      ],
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
    diffs: {
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
    loadRepositoryInventory: async () => undefined,
    loadDirectory: async () => undefined,
    loadDiff: async () => undefined,
  });

  const selectedOwner = chatIds[0]
    ? ({
        projectId: PROJECT_ID,
        chatId: chatIds[0],
        sessionId: SESSION_ID,
      } as AgentArtifactOwner)
    : null;
  if (
    scenario.mode === "agent" &&
    scenario.resource === "file" &&
    selectedOwner
  ) {
    useAgentArtifactAdmissionStore.getState().admit({
      owner: selectedOwner,
      tabId: "visual-file-tab",
    });
  }
  if (
    scenario.mode === "agent" &&
    scenario.resource === "diff" &&
    selectedOwner
  ) {
    useAgentArtifactAdmissionStore.getState().admit({
      owner: selectedOwner,
      tabId: "visual-diff-tab",
    });
  }
  setHarnessBrowseOwner(selectedOwner);
  if (scenario.resource === "preview" && selectedOwner) {
    applyBrowseIntent({
      owner: selectedOwner,
      url: `${window.location.origin}/tests/ui/fixtures/preview.html`,
    });
  }
}
