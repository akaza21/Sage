import type { AcpChatPage } from "../domain/acp";
import {
  agentDiffKey,
  parseAgentDiffRef,
  type AgentDiffRef,
} from "../domain/agentDiff";
import { sameResource } from "../domain/resourceIdentity";
import { projectAgentEditSummary } from "../features/chat/acp/agentEditSummary";
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

export function snapshotTabIsPresent(
  state: WorkbenchState,
  ref: AgentDiffRef,
): boolean {
  const key = agentDiffKey(ref);
  return state.tabs.some(
    (tab) =>
      tab.kind === "diff" &&
      !tab.repository &&
      sameResource(tab.resource, ref) &&
      tab.diffRef &&
      agentDiffKey(tab.diffRef) === key,
  );
}

/** A new card action must be the exact currently displayed retained projection. */
export function selectedSnapshotIsCurrent(
  state: WorkbenchState,
  value: AgentDiffRef,
): boolean {
  const ref = parseAgentDiffRef(value);
  if (!ref || snapshotProjectionRevision(state, ref) === null) return false;
  const page = acpProjectionStoreFor(ref.chatId).getState().page;
  const turn = page?.turns.find((candidate) => candidate.id === ref.turnId);
  if (!page || !turn) return false;
  return projectAgentEditSummary(page, turn).changes.some(
    (change) =>
      change.diffRef && agentDiffKey(change.diffRef) === agentDiffKey(ref),
  );
}
