import type {
  AgentArtifactAdmission,
  AgentArtifactOwner,
} from "../domain/agentArtifact";
import {
  sameAgentArtifactOwner,
  validAgentArtifactOwner,
} from "../domain/agentArtifact";

import { isShortId } from "../domain/guards";

export const MAX_AGENT_ADMITTED_ARTIFACTS = 32;

export function admitAgentArtifact(
  current: readonly AgentArtifactAdmission[],
  admission: AgentArtifactAdmission,
): AgentArtifactAdmission[] {
  if (
    !validAgentArtifactOwner(admission.owner) ||
    !isShortId(admission.tabId)
  ) {
    return [...current];
  }
  const next = current.filter(
    (candidate) =>
      candidate.tabId !== admission.tabId ||
      !sameAgentArtifactOwner(candidate.owner, admission.owner),
  );
  next.push(admission);
  if (next.length > MAX_AGENT_ADMITTED_ARTIFACTS) {
    next.shift();
  }
  return next;
}

export function forgetAgentArtifactTab(
  current: readonly AgentArtifactAdmission[],
  tabId: string,
): AgentArtifactAdmission[] {
  return current.filter((candidate) => candidate.tabId !== tabId);
}

export function agentArtifactVisible(input: {
  tabId: string;
  kind: string;
  admissions: readonly AgentArtifactAdmission[];
  focusedOwner: AgentArtifactOwner | null;
  hasFocusedChatStats: boolean;
}): boolean {
  if (input.kind === "browser") return true;
  if (input.kind !== "file" && input.kind !== "diff") return false;
  return (
    agentArtifactAdmitted(input.admissions, input.focusedOwner, input.tabId) ||
    input.hasFocusedChatStats
  );
}

export function agentArtifactAdmitted(
  current: readonly AgentArtifactAdmission[],
  owner: AgentArtifactOwner | null,
  tabId: string,
): boolean {
  return Boolean(
    owner &&
    current.some(
      (admission) =>
        admission.tabId === tabId &&
        sameAgentArtifactOwner(admission.owner, owner),
    ),
  );
}
