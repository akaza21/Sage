import { create } from "zustand";
import type { AgentDiffRef } from "../domain/agentDiff";
import type { AgentArtifactOwner } from "../domain/agentArtifact";
import { sameAgentArtifactOwner } from "../domain/agentArtifact";
import { useWorkbenchStore } from "./store";
import { acpProjectionStoreFor } from "./acpProjectionRegistry";
import { sameResource } from "../domain/resourceIdentity";
import { MAX_AGENT_ADMITTED_ARTIFACTS } from "../workbench/agentArtifactAdmission";

export const useAgentDiffSourceStore = create<{
  sourceReturns: readonly {
    tabId: string;
    ref: AgentDiffRef;
    generation: number;
    owner: AgentArtifactOwner;
  }[];
  rememberSource: (
    tabId: string,
    ref: AgentDiffRef,
    generation: number,
    owner: AgentArtifactOwner,
  ) => void;
}>((set) => ({
  sourceReturns: [],
  rememberSource: (tabId, ref, generation, owner) =>
    set((state) => ({
      sourceReturns: [
        ...state.sourceReturns.filter(
          (entry) =>
            entry.tabId !== tabId ||
            !sameAgentArtifactOwner(entry.owner, owner),
        ),
        { tabId, ref, generation, owner },
      ].slice(-MAX_AGENT_ADMITTED_ARTIFACTS),
    })),
}));

useWorkbenchStore.subscribe((state, previous) => {
  if (
    state.tabs === previous.tabs &&
    state.projectGeneration === previous.projectGeneration
  )
    return;
  const current = useAgentDiffSourceStore.getState().sourceReturns;
  const retained = current.filter(
    (entry) =>
      entry.generation === state.projectGeneration &&
      state.tabs.some((tab) => tab.id === entry.tabId),
  );
  if (retained.length !== current.length)
    useAgentDiffSourceStore.setState({ sourceReturns: retained });
});

/** Open the live File while keeping its immutable Changes return address. */
export async function openAgentSnapshotSource(
  ref: AgentDiffRef,
): Promise<boolean> {
  const { openAgentArtifact } = await import("./openAgentArtifact");
  const { selectedSnapshotIsCurrent } = await import("./agentDiffAdmission");
  const before = useWorkbenchStore.getState();
  if (!selectedSnapshotIsCurrent(before, ref)) return false;
  const sessionId = acpProjectionStoreFor(ref.chatId).getState().page
    ?.currentSessionId;
  if (!sessionId) return false;
  if (
    !(await openAgentArtifact({
      owner: { ...ref, sessionId },
      target: "file",
      select: true,
      resource: { rootId: ref.rootId, relativePath: ref.relativePath },
    }))
  )
    return false;
  const state = useWorkbenchStore.getState();
  if (
    state.projectGeneration !== before.projectGeneration ||
    !selectedSnapshotIsCurrent(state, ref)
  )
    return false;
  const tab = state.tabs.find(
    (candidate) =>
      candidate.kind === "file" && sameResource(candidate.resource, ref),
  );
  if (!tab) return false;
  useAgentDiffSourceStore
    .getState()
    .rememberSource(tab.id, ref, before.projectGeneration, {
      projectId: ref.projectId,
      chatId: ref.chatId,
      sessionId,
    });
  return true;
}
