import { agentDiffKey, type AgentDiffRef } from "../domain/agentDiff";
import { sameResource } from "../domain/resourceIdentity";
import type { WorkbenchState } from "./store";

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
