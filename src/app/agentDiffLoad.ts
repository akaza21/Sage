import {
  agentDiffKey,
  parseAgentDiffRef,
  type AgentDiffRef,
} from "../domain/agentDiff";
import { errorMessage } from "../domain/errors";
import { sameResource } from "../domain/resourceIdentity";
import type { ResourceRef } from "../domain/workbench";
import { snapshotFileDiff } from "../features/diff/snapshotDiff";
import { acpDesktop } from "../services/acpDesktop";
import {
  snapshotProjectionRevision,
  snapshotTabIsPresent,
} from "./agentDiffAdmission";
import { issueRequestIntent, matchesRequestIntent } from "./projectIntent";
import { boundedResourceStateUpdate } from "./resourceStateHelpers";
import type { WorkbenchState } from "./store";

/** One immutable read; never retries automatically on streaming projection updates. */
export async function runLoadAgentDiff(
  get: () => WorkbenchState,
  set: (partial: object | ((state: WorkbenchState) => object)) => void,
  resource: ResourceRef,
  value: AgentDiffRef,
): Promise<void> {
  const ref = parseAgentDiffRef(value);
  if (!ref || !sameResource(resource, ref) || !snapshotTabIsPresent(get(), ref))
    return;
  const admittedRef = ref;
  const tabIds = new Set(
    get()
      .tabs.filter(
        (tab) => tab.diffRef && agentDiffKey(tab.diffRef) === agentDiffKey(ref),
      )
      .map((tab) => tab.id),
  );
  const key = agentDiffKey(ref);
  const revision = snapshotProjectionRevision(get(), ref);
  const generation = get().projectGeneration;
  const transitionEpoch = get().nativeTransitionEpoch;
  const existing = get().diffs[key];
  if (revision === null) {
    set((state) => ({
      diffs: boundedResourceStateUpdate(
        state.diffs,
        key,
        {
          phase: "error",
          message:
            "This saved edit is unavailable for the current Chat session.",
        },
        32,
      ),
    }));
    return;
  }
  if (existing?.phase === "ready" || existing?.phase === "loading") return;
  const requestKey = `diff:${key}`;
  let token = 0;
  const loading = { phase: "loading" as const };
  set((state) => {
    const request = issueRequestIntent(
      state.requestGenerations,
      requestKey,
      state.requestIntentSequence,
    );
    token = request.token;
    return {
      requestGenerations: request.intents,
      requestIntentSequence: request.sequence,
      diffs: boundedResourceStateUpdate(state.diffs, key, loading, 32),
    };
  });
  function canPublish() {
    return (
      get().projectGeneration === generation &&
      !get().projectTransitioning &&
      get().project?.id === admittedRef.projectId &&
      snapshotTabIsPresent(get(), admittedRef) &&
      get().tabs.some((tab) => tabIds.has(tab.id)) &&
      matchesRequestIntent(get(), requestKey, token) &&
      get().diffs[key] === loading
    );
  }
  try {
    const snapshot = await acpDesktop.getFileSnapshot({
      ...ref,
      projectGeneration: String(transitionEpoch) as Parameters<
        typeof acpDesktop.getFileSnapshot
      >[0]["projectGeneration"],
      expectedProjectionRevision: revision,
    });
    if (!canPublish()) return;
    if (snapshotProjectionRevision(get(), ref) !== revision)
      throw new Error(
        "The Chat changed while loading this edit. Reopen the diff to retry.",
      );
    // Bridge validates digest/bytes; retain only the bounded presentation, not raw sides.
    const diff = snapshotFileDiff(resource, snapshot.oldText, snapshot.newText);
    set((state) => ({
      diffs: boundedResourceStateUpdate(
        state.diffs,
        key,
        { phase: "ready", value: diff },
        32,
      ),
    }));
  } catch (error) {
    if (!canPublish()) return;
    set((state) => ({
      diffs: boundedResourceStateUpdate(
        state.diffs,
        key,
        { phase: "error", message: errorMessage(error) },
        32,
      ),
    }));
  }
}
