import type { AcpChatPage } from "../../../../src/domain/acp";
import type { AgentArtifactOwner } from "../../../../src/domain/agentArtifact";
import type { WorkbenchTab } from "../../../../src/domain/workbench";
import { diffCacheKey } from "../../../../src/domain/diffIdentity";
import { useWorkbenchStore } from "../../../../src/app/store";
import { useAgentArtifactAdmissionStore } from "../../../../src/app/agentArtifactAdmissionStore";
import { useAgentArtifactFollowStore } from "../../../../src/app/agentArtifactFollowStore";
import { applyBrowseIntent } from "../../../../src/features/preview/harnessBrowse";

import { standardPage } from "../visualChatPages";
import {
  editorContent,
  editorDocument,
  fileResource,
} from "../visualDocumentFixtures";
import { PROJECT_ID, SESSION_ID } from "../visualFixtureIds";
import type {
  VisualAgentResourceState,
  VisualScenarioName,
} from "../visualScenarioTypes";

export function buildResourcePages(
  name: VisualScenarioName,
): AcpChatPage[] | null {
  switch (name) {
    case "file":
      return [
        standardPage("visual-chat-file", "Agent navigation polish", "rich"),
      ];
    case "diff":
      return [
        standardPage("visual-chat-diff", "Agent navigation polish", "rich"),
      ];
    case "preview":
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
    const key = diffCacheKey(resourceTab.repository, resourceTab.resource);
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
  const ownerA = {
    projectId: PROJECT_ID,
    chatId: chatA.id,
    sessionId: SESSION_ID,
  } as AgentArtifactOwner;
  const ownerB = {
    projectId: PROJECT_ID,
    chatId: chatB.id,
    sessionId: SESSION_ID,
  } as AgentArtifactOwner;
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
  applyBrowseIntent({
    owner: ownerB,
    url: `${window.location.origin}/tests/ui/fixtures/preview.html`,
  });
  await new Promise<void>((resolve) =>
    requestAnimationFrame(() => requestAnimationFrame(() => resolve())),
  );
}
