import type { SageAgentSessionId, SageChatId, SageProjectId } from "./acp";
import type { ResourceRef } from "./workbench";
import type { AgentDiffRef } from "./agentDiff";
import { isShortId } from "./guards";

export type AgentArtifactOwner = {
  projectId: SageProjectId;
  chatId: SageChatId;
  sessionId: SageAgentSessionId;
};

/** Pane selection also exists for historical Chats without a live Session. */
export type AgentPaneOwner = {
  projectId: string;
  chatId: string;
  sessionId: string | null;
};

export type AgentSubagentTarget = {
  owner: AgentArtifactOwner;
  itemId: string;
  agentKey: string;
};

export type AgentArtifactOpenIntent = {
  owner: AgentArtifactOwner;
  target: "file" | "diff" | "workingTreeDiff";
  resource: ResourceRef;
  diffRef?: AgentDiffRef;
};

export type AgentArtifactOpenHandler = (
  resource: ResourceRef,
  target: AgentArtifactOpenIntent["target"],
  origin?: AgentArtifactOwner,
  diffRef?: AgentDiffRef,
) => void;

export type AgentArtifactAdmission = {
  owner: AgentArtifactOwner;
  tabId: string;
};

export function validAgentArtifactOwner(
  owner: AgentPaneOwner | null | undefined,
): owner is AgentArtifactOwner {
  return validAgentPaneOwner(owner) && owner.sessionId !== null;
}

export function validAgentPaneOwner(
  owner: AgentPaneOwner | null | undefined,
): owner is AgentPaneOwner {
  return Boolean(
    owner &&
    isShortId(owner.projectId) &&
    isShortId(owner.chatId) &&
    (owner.sessionId === null || isShortId(owner.sessionId)),
  );
}

export function sameAgentArtifactOwner(
  left: AgentPaneOwner | null | undefined,
  right: AgentPaneOwner | null | undefined,
): boolean {
  return Boolean(
    left &&
    right &&
    left.projectId === right.projectId &&
    left.chatId === right.chatId &&
    left.sessionId === right.sessionId,
  );
}

export function agentArtifactOwnerKey(
  owner: AgentArtifactOwner | null | undefined,
): string | null {
  return validAgentArtifactOwner(owner)
    ? JSON.stringify([owner.projectId, owner.chatId, owner.sessionId])
    : null;
}
