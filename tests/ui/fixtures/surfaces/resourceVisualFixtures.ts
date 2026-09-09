import type { AcpChatPage } from "../../../../src/domain/acp";
import type { AgentArtifactOwner } from "../../../../src/domain/agentArtifact";
import type { HarnessBrowseOwner } from "../../../../src/features/preview/harnessBrowse";
import type { WorkbenchTab } from "../../../../src/domain/workbench";
import { diffCacheKey } from "../../../../src/domain/diffIdentity";
import { useWorkbenchStore } from "../../../../src/app/store";
import { useAgentArtifactAdmissionStore } from "../../../../src/app/agentArtifactAdmissionStore";
import { useAgentArtifactFollowStore } from "../../../../src/app/agentArtifactFollowStore";
import {
  applyBrowseIntent,
  clearBrowseSessions,
  setHarnessBrowseOwner,
} from "../../../../src/features/preview/harnessBrowse";

import { standardPage } from "../visualChatPages";
import {
  editorContent,
  editorDocument,
  fileResource,
  visualDiff,
  visualSnapshotDiffRef,
} from "../visualDocumentFixtures";
import {
  GENERATION,
  PROJECT_ID,
  RUNTIME_EPOCH,
  SESSION_ID,
} from "../visualFixtureIds";
import type {
  VisualAgentResourceState,
  VisualScenarioName,
} from "../visualScenarioTypes";

const VISUAL_NATIVE_TRANSITION_EPOCH = 0;

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

export function buildResourcePages(
  name: VisualScenarioName,
): AcpChatPage[] | null {
  switch (name) {
    case "file":
      return [
        standardPage("visual-chat-file", "Agent navigation polish", "rich"),
      ];
    case "diff": {
      const page = standardPage(
        "visual-chat-diff",
        "Agent navigation polish",
        "rich",
      );
      const changed = page.turns[0];
      if (!changed) throw new Error("Visual snapshot fixture requires a Turn");
      changed.items = changed.items.map((item) =>
        item.id === visualSnapshotDiffRef.itemId &&
        item.kind === "fileChangeDiff"
          ? {
              ...item,
              payload: {
                truncated: false,
                changes: [
                  {
                    resource: { kind: "resource", resource: fileResource },
                    operation: "updated",
                    originalResource: null,
                    summary: "Expanded the visible Chat limit",
                    additions: 1,
                    deletions: 1,
                    pairState: "available",
                    snapshotRevision: visualSnapshotDiffRef.snapshotRevision,
                    oldBytes: 54,
                    newBytes: 54,
                    contentDigest: visualSnapshotDiffRef.contentDigest,
                  },
                ],
              },
            }
          : item,
      );
      return [page];
    }
    case "preview":
    case "agent-preview-starting":
    case "agent-preview-health-failed":
    case "agent-preview-exited":
      return [
        standardPage("visual-chat-preview", "Preview interaction", "rich"),
      ];
    default:
      return null;
  }
}

export async function installVisualAgentResourceState(
  state: VisualAgentResourceState,
) {
  const current = useWorkbenchStore.getState();
  const expectedKind = state.startsWith("file") ? "file" : "diff";
  const resourceTab = current.tabs.find((tab) => tab.kind === expectedKind);
  if (!resourceTab?.resource) {
    throw new Error(`Visual ${expectedKind} state requires its resource tab`);
  }

  if (expectedKind === "file") {
    useWorkbenchStore.setState({
      activeEditor:
        state === "file-loading"
          ? { phase: "loading", resource: resourceTab.resource }
          : {
              phase: "error",
              resource: resourceTab.resource,
              message: "The file is temporarily unavailable.",
            },
    });
  } else {
    const key = diffCacheKey(
      resourceTab.repository,
      resourceTab.resource,
      resourceTab.diffRef,
    );
    useWorkbenchStore.setState({
      diffs: {
        ...current.diffs,
        [key]:
          state === "diff-loading"
            ? { phase: "loading" }
            : {
                phase: "error",
                message: "The comparison is temporarily unavailable.",
              },
      },
    });
  }

  document.documentElement.dataset.visualResourceState = state;
  await new Promise<void>((resolve) => requestAnimationFrame(() => resolve()));
}

export async function installVisualAgentArtifactFollowFixture() {
  const state = useWorkbenchStore.getState();
  const chats = state.tabs.filter((tab) => tab.kind === "chat");
  const chatA = chats[0];
  const chatB = chats[1];
  if (!chatA || !chatB || state.project?.id !== PROJECT_ID) {
    throw new Error("Visual Agent artifact fixture requires two Chats");
  }
  const ownerA = artifactOwner(chatA.id as AgentArtifactOwner["chatId"]);
  const ownerB = artifactOwner(chatB.id as AgentArtifactOwner["chatId"]);
  const browseOwnerB = browseOwner(chatB.id as HarnessBrowseOwner["chatId"]);
  const fileTab: WorkbenchTab = {
    id: "visual-follow-file-tab",
    kind: "file",
    title: "AgentChatPanes.tsx",
    position: chats.length,
    pinned: false,
    dirty: false,
    resource: fileResource,
  };
  const diffTab: WorkbenchTab = {
    ...fileTab,
    id: "visual-follow-diff-tab",
    kind: "diff",
    title: "AgentChatPanes.tsx changes",
    position: chats.length + 1,
  };
  useWorkbenchStore.setState({
    tabs: [...chats, fileTab, diffTab],
    activeTabId: fileTab.id,
    activeEditor: {
      phase: "ready",
      resource: fileResource,
      document: editorDocument(),
      content: editorContent,
      revision: 0,
      persistedRevision: 0,
      persisting: false,
      saving: false,
      message: null,
    },
  });
  useAgentArtifactAdmissionStore.getState().admit({
    owner: ownerA,
    tabId: diffTab.id,
  });
  useAgentArtifactFollowStore
    .getState()
    .pin(ownerA, { kind: "tab", tabId: diffTab.id });
  useAgentArtifactAdmissionStore.getState().admit({
    owner: ownerB,
    tabId: fileTab.id,
  });
  useAgentArtifactFollowStore
    .getState()
    .pin(ownerB, { kind: "tab", tabId: fileTab.id });
  setHarnessBrowseOwner(browseOwnerB, [chatA.id, chatB.id]);
  applyBrowseIntent({
    owner: browseOwnerB,
    url: visualPreviewUrl(),
  });
  await new Promise<void>((resolve) =>
    requestAnimationFrame(() => requestAnimationFrame(() => resolve())),
  );
}

export async function installVisualAgentPreviewIdleFixture() {
  const state = useWorkbenchStore.getState();
  const chats = state.tabs.filter((tab) => tab.kind === "chat");
  const chat = chats[0];
  if (!chat || state.project?.id !== PROJECT_ID) {
    throw new Error("Visual Preview idle fixture requires a Chat");
  }
  const owner = artifactOwner(chat.id as AgentArtifactOwner["chatId"]);
  const previewOwner = browseOwner(chat.id as HarnessBrowseOwner["chatId"]);
  clearBrowseSessions();
  setHarnessBrowseOwner(previewOwner, [chat.id]);
  useAgentArtifactFollowStore.getState().pin(owner, { kind: "preview" });
  await new Promise<void>((resolve) =>
    requestAnimationFrame(() => requestAnimationFrame(() => resolve())),
  );
}

const LONG_PATH =
  "src/workbench/nested/panels/with/very/long/directory/names/for/ellipsis/AgentChatPanes.tsx";

export async function installVisualAgentLongPathFixture() {
  const state = useWorkbenchStore.getState();
  const chats = state.tabs.filter((tab) => tab.kind === "chat");
  const chat = chats[0];
  if (!chat || state.project?.id !== PROJECT_ID) {
    throw new Error("Visual long-path fixture requires a Chat");
  }
  const owner = artifactOwner(chat.id as AgentArtifactOwner["chatId"]);
  const longResource = { rootId: "visual-root", relativePath: LONG_PATH };
  const fileTab: WorkbenchTab = {
    id: "visual-long-file-tab",
    kind: "file",
    title: "AgentChatPanes.tsx",
    position: state.tabs.length,
    pinned: false,
    dirty: false,
    resource: longResource,
  };
  const diffTab: WorkbenchTab = {
    id: "visual-long-diff-tab",
    kind: "diff",
    title: "AgentChatPanes.tsx",
    position: state.tabs.length + 1,
    pinned: false,
    dirty: false,
    resource: longResource,
  };
  useWorkbenchStore.setState({
    tabs: [...state.tabs, fileTab, diffTab],
  });
  const longDocument = { ...editorDocument(), resource: longResource };
  useWorkbenchStore.setState({
    activeEditor: {
      phase: "ready",
      resource: longResource,
      document: longDocument,
      content: editorContent,
      revision: 0,
      persistedRevision: 0,
      persisting: false,
      saving: false,
      message: null,
    },
    diffs: {
      ...useWorkbenchStore.getState().diffs,
      [diffCacheKey(undefined, longResource)]: {
        phase: "ready",
        value: { ...visualDiff, resource: longResource },
      },
    },
  });
  useAgentArtifactAdmissionStore.getState().admit({
    owner,
    tabId: fileTab.id,
  });
  useAgentArtifactAdmissionStore.getState().admit({
    owner,
    tabId: diffTab.id,
  });
  useAgentArtifactFollowStore
    .getState()
    .pin(owner, { kind: "tab", tabId: fileTab.id });
  await new Promise<void>((resolve) =>
    requestAnimationFrame(() => requestAnimationFrame(() => resolve())),
  );
}
