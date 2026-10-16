import type { AcpChatPage } from "../domain/acp";
import type { AgentDiffRef } from "../domain/agentDiff";
import { acpProjectionStoreFor } from "./acpProjectionRegistry";
import type { AcpProjectionScope } from "./acpProjectionStore";
import type { WorkbenchState } from "./store";

export function snapshotProjectionRevision(
  state: Pick<
    WorkbenchState,
    "project" | "projectGeneration" | "projectTransitioning"
  >,
  ref: AgentDiffRef,
  projection = acpProjectionStoreFor(ref.chatId).getState() as {
    scope: AcpProjectionScope | null;
    page: AcpChatPage | null;
  },
) {
  const { scope, page } = projection;
  if (
    state.projectTransitioning ||
    state.project?.id !== ref.projectId ||
    !state.project.roots.some((root) => root.id === ref.rootId) ||
    scope?.projectId !== ref.projectId ||
    scope.chatId !== ref.chatId ||
    scope.projectGeneration !== String(state.projectGeneration) ||
    page?.projectId !== ref.projectId ||
    page.chatId !== ref.chatId ||
    page.currentSessionId !== ref.sessionId
  )
    return null;
  return page.projectionRevision;
}
