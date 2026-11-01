import type { SageAgentSessionId, SageChatId, SageProjectId } from "./acp";
import type { ResourceRef } from "./workbench";
import type { AgentDiffRef } from "./agentDiff";
import { isShortId } from "./guards";

export type AgentArtifactOwner = {
  projectId: SageProjectId;
  chatId: SageChatId;
  sessionId: SageAgentSessionId;
};

export type AgentArtifactOpenIntent = {
  owner: AgentArtifactOwner;
  target: "file" | "diff";
  resource: ResourceRef;
  diffRef?: AgentDiffRef;
  select?: boolean;
};

export type AgentArtifactOpenHandler = (
  resource: ResourceRef,
  target: "file" | "diff",
  origin?: AgentArtifactOwner,
  diffRef?: AgentDiffRef,
) => void;

export type AgentArtifactAdmission = {
  owner: AgentArtifactOwner;
  tabId: string;
};

export function validAgentArtifactOwner(
  owner: AgentArtifactOwner | null | undefined,
): owner is AgentArtifactOwner {
  return Boolean(
    owner &&
    isShortId(owner.projectId) &&
    isShortId(owner.chatId) &&
    isShortId(owner.sessionId),
  );
}

export function sameAgentArtifactOwner(
  left: AgentArtifactOwner | null | undefined,
  right: AgentArtifactOwner | null | undefined,
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
