import {
  registerNavigationPanes,
  type NavigationPane,
} from "./navigationHistory";
import type { AgentArtifactOwner } from "../domain/agentArtifact";
import { pinnedAgentChatOwner } from "../app/agentChatOwner";
import { useAgentArtifactFollowStore } from "../app/agentArtifactFollowStore";
import { useSubagentPaneStore } from "../app/subagentPaneStore";
import { useWorkbenchStore } from "../app/store";
import { useAgentChangesState } from "./agentChangesState";

export function captureNavigationPane(): NavigationPane | null {
  const state = useWorkbenchStore.getState();
  const active = state.tabs.find((tab) => tab.id === state.activeTabId);
  const chatId = active?.kind === "chat" ? active.id : pinnedAgentChatOwner();
  if (!chatId) return null;
  const matches = (owner: { chatId: string; projectId: string } | null) =>
    owner?.chatId === chatId && owner.projectId === state.project?.id;
  const changes = useAgentChangesState.getState();
  if (
    changes.selected &&
    changes.generation === state.projectGeneration &&
    matches(changes.owner)
  ) {
    return {
      kind: "changes",
      chatId,
      sessionId: changes.owner!.sessionId,
      targetId: null,
      agentKey: null,
    };
  }
  const subagent = useSubagentPaneStore.getState();
  if (subagent.selected && subagent.open && matches(subagent.open.owner)) {
    return {
      kind: "subagent",
      chatId,
      sessionId: subagent.open.owner.sessionId,
      targetId: subagent.open.itemId,
      agentKey: subagent.open.agentKey,
    };
  }
  const record = useAgentArtifactFollowStore
    .getState()
    .records.find((record) => matches(record.owner));
  if (!record?.selected) return null;
  return {
    kind: record.selected.kind,
    chatId,
    sessionId: record.owner.sessionId,
    targetId: record.selected.kind === "tab" ? record.selected.tabId : null,
    agentKey: null,
  };
}
export function restoreNavigationPane(pane: NavigationPane | null) {
  const state = useWorkbenchStore.getState();
  useAgentChangesState.getState().select(false);
  useSubagentPaneStore.getState().select(false);
  if (!pane) {
    const active = state.tabs.find((tab) => tab.id === state.activeTabId);
    const chatId = active?.kind === "chat" ? active.id : pinnedAgentChatOwner();
    useAgentArtifactFollowStore.setState(({ records }) => ({
      records: records.map((record) =>
        record.owner.chatId === chatId &&
        record.owner.projectId === state.project?.id
          ? { ...record, selected: null }
          : record,
      ),
    }));
    return;
  }
  if (!state.project) return;
  const owner = {
    projectId: state.project.id,
    chatId: pane.chatId,
    sessionId: pane.sessionId,
  };
  if (pane.kind === "changes") {
    useAgentChangesState.getState().show(owner, state.projectGeneration);
  } else if (owner.sessionId) {
    const artifactOwner = {
      ...owner,
      sessionId: owner.sessionId,
    } as AgentArtifactOwner;
    if (pane.kind === "subagent" && pane.targetId && pane.agentKey) {
      useSubagentPaneStore.getState().show({
        owner: artifactOwner,
        itemId: pane.targetId,
        agentKey: pane.agentKey,
      });
    } else if (
      pane.kind === "preview" ||
      (pane.kind === "tab" && pane.targetId)
    ) {
      useAgentArtifactFollowStore
        .getState()
        .pin(
          artifactOwner,
          pane.kind === "preview"
            ? { kind: "preview" }
            : { kind: "tab", tabId: pane.targetId! },
        );
    }
  }
}
export function subscribeNavigationPanes(observe: () => void) {
  const stops = [
    useAgentChangesState.subscribe(observe),
    useSubagentPaneStore.subscribe(observe),
    useAgentArtifactFollowStore.subscribe(observe),
  ];
  return () => stops.forEach((stop) => stop());
}

// Registers code only. The Workbench lifetime owns all store subscriptions.
registerNavigationPanes({
  capture: captureNavigationPane,
  restore: restoreNavigationPane,
  subscribe: subscribeNavigationPanes,
});
