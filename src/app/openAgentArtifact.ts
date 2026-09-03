import { acpProjectionStoreFor } from "./acpProjectionRegistry";
import { useAgentArtifactAdmissionStore } from "./agentArtifactAdmissionStore";
import { useAgentArtifactFollowStore } from "./agentArtifactFollowStore";
import { useWorkbenchStore } from "./store";
import type {
  AgentArtifactOpenIntent,
  AgentArtifactOwner,
} from "../domain/agentArtifact";
import { validAgentArtifactOwner } from "../domain/agentArtifact";
import { sameRepository } from "../domain/diffIdentity";
import { sameResource } from "../domain/resourceIdentity";

export function agentArtifactOwnerIsCurrent(
  owner: AgentArtifactOwner,
  projectGeneration: number,
): boolean {
  const workbench = useWorkbenchStore.getState();
  if (
    workbench.projectTransitioning ||
    workbench.projectGeneration !== projectGeneration ||
    workbench.project?.id !== owner.projectId
  ) {
    return false;
  }
  const projection = acpProjectionStoreFor(owner.chatId).getState();
  const scope = projection.scope;
  const page = projection.page;
  return Boolean(
    scope &&
    scope.projectId === owner.projectId &&
    scope.chatId === owner.chatId &&
    scope.projectGeneration === String(projectGeneration) &&
    page &&
    page.projectId === owner.projectId &&
    page.chatId === owner.chatId &&
    page.currentSessionId === owner.sessionId,
  );
}

export async function openAgentArtifact(
  intent: AgentArtifactOpenIntent,
): Promise<boolean> {
  if (!validAgentArtifactOwner(intent.owner)) return false;
  const projectGeneration = useWorkbenchStore.getState().projectGeneration;
  if (!agentArtifactOwnerIsCurrent(intent.owner, projectGeneration)) {
    return false;
  }

  if (intent.target === "file") {
    await useWorkbenchStore.getState().openFile(intent.resource);
  } else {
    await useWorkbenchStore.getState().openDiff(intent.resource);
  }

  if (!agentArtifactOwnerIsCurrent(intent.owner, projectGeneration)) {
    return false;
  }
  const tab = useWorkbenchStore
    .getState()
    .tabs.find(
      (candidate) =>
        candidate.kind === intent.target &&
        sameResource(candidate.resource, intent.resource) &&
        (intent.target === "file" ||
          sameRepository(candidate.repository, undefined)),
    );
  if (!tab) return false;
  useAgentArtifactAdmissionStore.getState().admit({
    owner: intent.owner,
    tabId: tab.id,
  });
  useAgentArtifactFollowStore.getState().pin(intent.owner, {
    kind: "tab",
    tabId: tab.id,
  });
  return true;
}
