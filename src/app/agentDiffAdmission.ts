import {
  agentDiffKey,
  parseAgentDiffRef,
  type AgentDiffRef,
} from "../domain/agentDiff";
import { sameResource } from "../domain/resourceIdentity";
import { projectAgentEditSummary } from "../features/chat/acp/agentEditSummary";
import { acpProjectionStoreFor } from "./acpProjectionRegistry";
import type { WorkbenchState } from "./store";
import { snapshotProjectionRevision } from "./snapshotProjection";

export { snapshotProjectionRevision } from "./snapshotProjection";

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
