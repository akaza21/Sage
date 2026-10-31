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
import { sameAgentArtifactOwner } from "../domain/agentArtifact";
import { agentDiffKey } from "../domain/agentDiffIdentity";
import { useAgentChangesState } from "../workbench/agentChangesState";
import { useSubagentPaneStore } from "./subagentPaneStore";

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
  if (
    intent.target === "file" &&
    !agentArtifactOwnerIsCurrent(intent.owner, projectGeneration)
  ) {
    return false;
  }

  if (intent.target === "file") {
    await useWorkbenchStore.getState().openFile(intent.resource);
  } else {
    const { selectedSnapshotIsCurrent, snapshotProjectionRevision } =
      await import("./agentDiffAdmission");
    const ref = intent.diffRef;
    if (
      !ref ||
      !sameAgentArtifactOwner(intent.owner, ref) ||
      !sameResource(intent.resource, ref) ||
      !selectedSnapshotIsCurrent(useWorkbenchStore.getState(), ref)
    )
      return false;
    const revision = snapshotProjectionRevision(
      useWorkbenchStore.getState(),
      ref,
    );
    await useWorkbenchStore
      .getState()
      .openDiff(intent.resource, undefined, ref);
    if (
      snapshotProjectionRevision(useWorkbenchStore.getState(), ref) !==
        revision ||
      !selectedSnapshotIsCurrent(useWorkbenchStore.getState(), ref)
    )
      return false;
  }

  // Historical bytes retain their original session identity. Presentation is
  // admitted to the current Chat pane without granting old runtime authority.
  const currentSessionId = acpProjectionStoreFor(intent.owner.chatId).getState()
    .page?.currentSessionId;
  const displayOwner =
    intent.target === "diff" && currentSessionId
      ? { ...intent.owner, sessionId: currentSessionId }
      : intent.owner;
  if (!agentArtifactOwnerIsCurrent(displayOwner, projectGeneration)) {
    return false;
  }
  const tab = useWorkbenchStore
    .getState()
    .tabs.find(
      (candidate) =>
        candidate.kind === intent.target &&
        sameResource(candidate.resource, intent.resource) &&
        (intent.target === "file" ||
          (sameRepository(candidate.repository, undefined) &&
            candidate.diffRef &&
            intent.diffRef &&
            agentDiffKey(candidate.diffRef) === agentDiffKey(intent.diffRef))),
    );
  if (!tab) return false;
  useAgentArtifactAdmissionStore.getState().admit({
    owner: displayOwner,
    tabId: tab.id,
  });
  useAgentArtifactFollowStore.getState().observe(displayOwner, {
    kind: "tab",
    tabId: tab.id,
  });
  const changes = useAgentChangesState.getState();
  const subagent = useSubagentPaneStore.getState();
  const changesSelected =
    changes.selected &&
    changes.owner?.projectId === displayOwner.projectId &&
    changes.owner.chatId === displayOwner.chatId &&
    changes.owner.sessionId === displayOwner.sessionId;
  if (
    changesSelected ||
    (subagent.selected &&
      sameAgentArtifactOwner(subagent.open?.owner, displayOwner))
  ) {
    changes.select(false);
    subagent.select(false);
    useAgentArtifactFollowStore
      .getState()
      .pin(displayOwner, { kind: "tab", tabId: tab.id });
  }
  return true;
}
