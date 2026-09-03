import type { SageAgentSessionId, SageChatId, SageProjectId } from "./acp";
import type { ResourceRef } from "./workbench";

export type AgentArtifactOwner = {
  projectId: SageProjectId;
  chatId: SageChatId;
  sessionId: SageAgentSessionId;
};

export type AgentArtifactOpenIntent = {
  owner: AgentArtifactOwner;
  target: "file" | "diff";
  resource: ResourceRef;
};

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
