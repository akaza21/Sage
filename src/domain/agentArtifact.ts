import type { SageAgentSessionId, SageChatId, SageProjectId } from "./acp";
import type { ResourceRef } from "./workbench";
import type { AgentDiffRef } from "./agentDiff";

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

const MAX_AGENT_ARTIFACT_ID_BYTES = 512;

function safeIdentityPart(value: string): boolean {
  return (
    value.length > 0 &&
    value.length <= MAX_AGENT_ARTIFACT_ID_BYTES &&
    !value.includes("\0")
  );
}

export function validAgentArtifactOwner(
  owner: AgentArtifactOwner | null | undefined,
): owner is AgentArtifactOwner {
  return Boolean(
    owner &&
    safeIdentityPart(owner.projectId) &&
    safeIdentityPart(owner.chatId) &&
    safeIdentityPart(owner.sessionId),
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
