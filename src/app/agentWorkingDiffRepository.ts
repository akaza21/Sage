import type { AgentArtifactOpenIntent } from "../domain/agentArtifact";
import { errorMessage } from "../domain/errors";
import { getWorkbenchPorts } from "../ports/workbenchRuntime";
import { agentArtifactOwnerIsCurrent } from "./openAgentArtifact";
import { useWorkbenchStore } from "./store";

/** Repository lookup is needed only when opening a summary-only working Diff. */
export async function resolveAgentWorkingDiffRepository(
  intent: AgentArtifactOpenIntent,
  projectGeneration: number,
) {
  if (!agentArtifactOwnerIsCurrent(intent.owner, projectGeneration))
    return null;
  const expectedTransitionEpoch =
    useWorkbenchStore.getState().nativeTransitionEpoch;
  try {
    const result =
      await getWorkbenchPorts().repository.resolveResourceRepository({
        projectId: intent.owner.projectId,
        projectGeneration,
        expectedTransitionEpoch,
        resource: intent.resource,
      });
    if (
      !agentArtifactOwnerIsCurrent(intent.owner, projectGeneration) ||
      useWorkbenchStore.getState().nativeTransitionEpoch !==
        expectedTransitionEpoch ||
      result.projectId !== intent.owner.projectId ||
      result.transitionEpoch !== expectedTransitionEpoch
    )
      return null;
    return { repository: result.repository ?? undefined };
  } catch (error) {
    if (agentArtifactOwnerIsCurrent(intent.owner, projectGeneration))
      useWorkbenchStore.setState({ error: errorMessage(error) });
    return null;
  }
}
