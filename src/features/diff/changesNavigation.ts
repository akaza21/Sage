import { acpProjectionStoreFor } from "../../app/acpProjectionRegistry";
import { useWorkbenchStore } from "../../app/store";
import { openAgentArtifact } from "../../app/openAgentArtifact";
import { openAgentSnapshotSource } from "../../app/agentDiffSource";
import { resourceTabUpdate } from "../../app/resourceTabs";
import { useAgentArtifactAdmissionStore } from "../../app/agentArtifactAdmissionStore";
import { useAgentArtifactFollowStore } from "../../app/agentArtifactFollowStore";
import { useAgentChangesState } from "../../workbench/agentChangesState";
import { sameResource } from "../../domain/resourceIdentity";
import type { AcpChatPage } from "../../domain/acpProjection";
import type { ChangesEntry, ChangesScope } from "./agentChanges";

export async function openChangesEntry(
  entry: ChangesEntry,
  source: boolean,
  page: AcpChatPage | null,
  scope: ChangesScope,
) {
  const before = useWorkbenchStore.getState();
  const changesOwner = useAgentChangesState.getState().owner;
  if (
    before.projectTransitioning ||
    (before.project?.id !== page?.projectId && scope === "lastTurn")
  )
    return;

  if (entry.diffRef) {
    if (
      !(source
        ? await openAgentSnapshotSource(entry.diffRef)
        : await openAgentArtifact({
            owner: entry.diffRef,
            target: "diff",
            select: true,
            resource: entry.resource,
            diffRef: entry.diffRef,
          }))
    )
      throw new Error(
        "This saved change is no longer available. Refresh changes.",
      );
    return;
  }
  if (source) await before.openFile(entry.resource);
  else {
    if (
      before.fileCreateInFlight ||
      before.folderCreateInFlight ||
      before.fileRenameInFlight ||
      before.resourceTrashInFlight
    )
      return;
    await before.flushActiveEditor();
    const current = useWorkbenchStore.getState();
    if (
      current.projectTransitioning ||
      current.projectGeneration !== before.projectGeneration ||
      current.project?.id !== before.project?.id ||
      !current.project?.roots.some(
        (root) => root.id === entry.resource.rootId,
      ) ||
      useAgentChangesState.getState().owner !== changesOwner
    )
      return;
    useWorkbenchStore.setState(
      resourceTabUpdate(
        current,
        "diff",
        entry.resource,
        undefined,
        undefined,
        entry.gitScope,
      ),
    );
    const retained = useWorkbenchStore.getState().tabs;
    for (const old of current.tabs)
      if (!retained.some((tab) => tab.id === old.id)) {
        useAgentArtifactAdmissionStore.getState().forget(old.id);
        useAgentArtifactFollowStore.getState().forgetTab(old.id);
      }
  }
  const state = useWorkbenchStore.getState();
  if (
    state.projectTransitioning ||
    state.projectGeneration !== before.projectGeneration ||
    state.project?.id !== before.project?.id ||
    useAgentChangesState.getState().owner !== changesOwner
  )
    return;
  const tab = state.tabs.find(
    (candidate) =>
      candidate.id === state.activeTabId &&
      candidate.kind === (source ? "file" : "diff") &&
      sameResource(candidate.resource, entry.resource) &&
      !candidate.diffRef,
  );
  if (!tab) return;
  if (!source && entry.gitScope) {
    useWorkbenchStore.setState({ activeEditor: null });
    void state.revealFile(entry.resource);
  }
  const owner = useAgentChangesState.getState().owner;
  if (owner?.sessionId && owner.projectId === state.project?.id) {
    const livePage = acpProjectionStoreFor(owner.chatId).getState().page;
    if (
      !livePage ||
      livePage.projectId !== owner.projectId ||
      livePage.chatId !== owner.chatId ||
      livePage.currentSessionId !== owner.sessionId
    )
      return;
    const displayOwner = {
      projectId: livePage.projectId,
      chatId: livePage.chatId,
      sessionId: livePage.currentSessionId,
    };
    useAgentArtifactAdmissionStore
      .getState()
      .admit({ owner: displayOwner, tabId: tab.id });
    useAgentArtifactFollowStore
      .getState()
      .pin(displayOwner, { kind: "tab", tabId: tab.id });
  }
  useAgentChangesState.getState().select(false);
}
