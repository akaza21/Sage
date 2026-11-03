import type { AcpChatPage } from "../../../../src/domain/acp";
import {
  childResource,
  resourceKey,
} from "../../../../src/domain/resourceIdentity";
import { useWorkbenchStore } from "../../../../src/app/store";
import { getWorkbenchPorts } from "../../../../src/ports/workbenchRuntime";
import { workspaceSearchDesktop } from "../../../../src/features/search/workspaceSearchDesktop";

import { standardPage } from "../visualChatPages";
import { fileResource, visualFileGitBase } from "../visualDocumentFixtures";
import { ROOT_ID } from "../visualFixtureIds";
import type { VisualScenarioName } from "../visualScenarioTypes";

const OUTLINE_LONG_CONTENT = [
  "export function AgentChatPanes() {}",
  "export type AgentChatPanesWithProjectScopedResourceFollowing = string;",
  "const visibleChatsForTheActiveProjectWithinTheBoundedPairLimit = 2;",
].join("\n");
const OUTLINE_EMPTY_CONTENT =
  'import { ChatSurface } from "../features/chat/ChatSurface";\n';

export function buildEditorPages(
  name: VisualScenarioName,
): AcpChatPage[] | null {
  if (
    ![
      "editor",
      "editor-picker-repository",
      "editor-picker-repository-filtered",
      "editor-picker-narrow",
      "editor-picker-zoom-200",
    ].includes(name)
  ) {
    return null;
  }
  return [
    standardPage("visual-chat-editor", "Agent navigation polish", "rich"),
  ];
}

export function installVisualRepositoryPickerFixture({
  disabled = false,
}: {
  disabled?: boolean;
} = {}) {
  useWorkbenchStore.setState((state) => ({
    repositoryRefs: Array.from({ length: 32 }, (_, index) => ({
      rootId: index === 31 ? "visual-readonly" : ROOT_ID,
      relativePath: index === 0 ? "" : `packages/repository-${index}`,
    })),
    project: state.project
      ? {
          ...state.project,
          roots: [
            ...state.project.roots,
            {
              id: "visual-readonly",
              name: "Read-only root",
              ordinal: state.project.roots.length,
              availability: "unavailable",
              message: "Repository root is unavailable.",
            },
          ],
        }
      : state.project,
    selectedRepository: { rootId: ROOT_ID, relativePath: "" },
    repositoryInventoryPhase: disabled ? "loading" : "ready",
    repositoryInventoryError: null,
    repositoryStatusPhase: "ready",
    repositoryStatusError: null,
    repositoryRefreshing: false,
    projectTransitioning: false,
    repositoryMutation: null,
    repositoryCommit: null,
    repositoryCheckout: null,
    repositoryRemote: null,
    repositories: state.repositories,
    selectRepository: async (repository) => {
      useWorkbenchStore.setState({ selectedRepository: repository });
    },
  }));
}

export function installEditorToolFixtures() {
  getWorkbenchPorts().repository.getFileGitBase = async (resource, knownOid) =>
    visualFileGitBase(resource, knownOid);
  workspaceSearchDesktop.search = async (request) => ({
    requestId: "visual-search",
    projectId: request.projectId,
    transitionEpoch: request.expectedTransitionEpoch,
    indexGeneration: request.indexGeneration,
    query: request.query,
    matches: [
      {
        resource: fileResource,
        lineNumber: 3,
        columnNumber: 17,
        preview: "export function AgentChatPanes() {",
      },
    ],
    roots: [
      {
        rootId: ROOT_ID,
        rootName: "Sage",
        state: "complete",
        filesScanned: 124,
        bytesScanned: 84_200,
        entriesVisited: 139,
        skippedFiles: 0,
        errorCount: 0,
        message: null,
      },
    ],
    truncated: false,
    continuationToken: null,
    limitReasons: [],
    elapsedMs: 8,
  });
}

export async function installVisualExplorerScrollFixture() {
  const state = useWorkbenchStore.getState();
  const workbench = childResource(ROOT_ID, "src/workbench");
  const key = resourceKey(workbench);
  const listing = state.directoryListings[key];
  if (listing?.phase !== "ready") {
    throw new Error(
      "Visual Explorer scroll fixture requires workbench listing",
    );
  }
  const filler = Array.from({ length: 40 }, (_, index) => ({
    name: `fixture-${String(index).padStart(2, "0")}.ts`,
    resource: childResource(
      ROOT_ID,
      `src/workbench/fixture-${String(index).padStart(2, "0")}.ts`,
    ),
    kind: "file" as const,
    byteLen: 40 + index,
  }));
  useWorkbenchStore.setState({
    selectedResource: null,
    directoryListings: {
      ...state.directoryListings,
      [key]: {
        phase: "ready",
        value: {
          ...listing.value,
          entries: [...filler, ...listing.value.entries],
        },
      },
    },
  });
  await new Promise<void>((resolve) => requestAnimationFrame(() => resolve()));
  useWorkbenchStore.setState({ selectedResource: fileResource });
  await new Promise<void>((resolve) =>
    requestAnimationFrame(() => requestAnimationFrame(() => resolve())),
  );
}

export function installVisualExplorerActionFixture() {
  useWorkbenchStore.setState({
    selectedResource: childResource(ROOT_ID, "README.md"),
  });
}

export function installVisualExplorerUnavailableActionFixture() {
  useWorkbenchStore.setState({ selectedResource: null });
}

export function installVisualRepositoryError() {
  useWorkbenchStore.setState({
    repositoryInventoryError: "Could not refresh repository inventory.",
    repositoryInventoryPhase: "ready",
    repositoryStatusPhase: "ready",
  });
}

export function installVisualRepositoryLoading() {
  useWorkbenchStore.setState({
    repositoryInventoryError: null,
    repositoryStatusError: null,
    repositoryInventoryPhase: "loading",
    repositoryStatusPhase: "loading",
  });
}

export function installVisualRepositoryPrimaryState(
  primaryState: "ahead" | "behind" | "diverged",
) {
  const divergence = {
    ahead: { ahead: 2, behind: 0 },
    behind: { ahead: 0, behind: 1 },
    diverged: { ahead: 2, behind: 1 },
  }[primaryState];
  useWorkbenchStore.setState((state) => ({
    repositories: {
      ...state.repositories,
      roots: state.repositories.roots.map((repository) => ({
        ...repository,
        upstream: "origin/feature/editor-polish",
        changes: [],
        truncated: false,
        ...divergence,
      })),
    },
  }));
}

/** Leaves the active File open with no declarations, so Outline is empty. */
export function installVisualEmptyEditorTools() {
  useWorkbenchStore.setState((state) =>
    state.activeEditor?.phase === "ready"
      ? {
          activeEditor: {
            ...state.activeEditor,
            content: OUTLINE_EMPTY_CONTENT,
          },
        }
      : {},
  );
}

/** Declarations long enough to exercise Outline row truncation. */
export function installVisualLongOutlineSymbols() {
  useWorkbenchStore.setState((state) =>
    state.activeEditor?.phase === "ready"
      ? {
          activeEditor: {
            ...state.activeEditor,
            content: OUTLINE_LONG_CONTENT,
          },
        }
      : {},
  );
}

export function installVisualEmptyEditorCanvas() {
  useWorkbenchStore.setState((state) => ({
    tabs: state.tabs.filter((tab) => tab.kind === "chat"),
    activeTabId: state.tabs.find((tab) => tab.kind === "chat")?.id ?? null,
    activeEditor: null,
    selectedResource: null,
  }));
}

export function installVisualDirtyEditor() {
  useWorkbenchStore.setState((state) => ({
    tabs: state.tabs.map((tab) =>
      tab.kind === "file" ? { ...tab, dirty: true } : tab,
    ),
    activeEditor:
      state.activeEditor?.phase === "ready"
        ? (() => {
            const content = `${state.activeEditor.content}\n// interaction-state review`;
            const revision = state.activeEditor.revision + 1;
            return {
              ...state.activeEditor,
              document: {
                ...state.activeEditor.document,
                buffer: {
                  ...state.activeEditor.document.buffer,
                  fingerprint: "visual-editor-dirty-v1",
                  byteLen: new TextEncoder().encode(content).byteLength,
                  revision,
                  content,
                },
                dirty: true,
              },
              content,
              revision,
              // This fixture captures the post-recovery-snapshot dirty state;
              // disk persistence still requires the explicit Save command.
              persistedRevision: revision,
            };
          })()
        : state.activeEditor,
  }));
}
