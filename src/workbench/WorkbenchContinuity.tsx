import { useEffect, useRef } from "react";
import { usePinnedAgentChatOwner } from "../app/agentChatOwner";
import { useWorkbenchStore } from "../app/store";
import { admitsFocusedCodexRendererRecreation } from "../features/chat/acp/rendererRecreationCollectorAdmission";
import { mountRendererRecreationPacketCollector } from "../features/chat/acp/rendererRecreationPacket";
import "./harness-browser-mode.css";
import "./workbench-window-mode.css";
import {
  agentCenterNeedsDraft,
  isAgentArtifactTabKind,
  lastArtifactTabId,
  type WorkbenchWindowMode,
} from "./workbenchWindowMode";

export function EditorContinuityKeeper({
  windowMode,
}: {
  windowMode: WorkbenchWindowMode;
}) {
  const tabs = useWorkbenchStore((state) => state.tabs);
  const activeTabId = useWorkbenchStore((state) => state.activeTabId);
  const lastArtifactId = useRef<string | null>(null);
  const previousMode = useRef<WorkbenchWindowMode | null>(null);

  useEffect(() => {
    const active = tabs.find((tab) => tab.id === activeTabId);
    if (active && isAgentArtifactTabKind(active.kind)) {
      lastArtifactId.current = active.id;
    }
  }, [activeTabId, tabs]);

  useEffect(() => {
    const enteredEditor =
      windowMode === "editor" && previousMode.current !== "editor";
    previousMode.current = windowMode;
    if (!enteredEditor) return;
    useWorkbenchStore.setState({ pendingFirstSend: null });
    const state = useWorkbenchStore.getState();
    const active =
      state.tabs.find((tab) => tab.id === state.activeTabId) ?? null;
    if (
      active &&
      (isAgentArtifactTabKind(active.kind) || active.kind === "settings")
    )
      return;
    const continuity = lastArtifactTabId(state.tabs, lastArtifactId.current);
    if (continuity && continuity !== state.activeTabId) {
      void state.activateTab(continuity);
    }
  }, [windowMode]);

  return null;
}

export function AgentCenterKeeper({
  windowMode,
}: {
  windowMode: WorkbenchWindowMode;
}) {
  const focusedChatId = usePinnedAgentChatOwner();
  const projectId = useWorkbenchStore((state) =>
    windowMode === "agent" &&
    state.project &&
    !state.projectTransitioning &&
    state.agentCenter !== "draft" &&
    agentCenterNeedsDraft(state.tabs, state.activeTabId, focusedChatId)
      ? state.project.id
      : null,
  );
  useEffect(() => {
    const state = useWorkbenchStore.getState();
    if (projectId && state.project?.id === projectId)
      void state.openNewChatDraft(
        state.project.isInternal ? undefined : projectId,
      );
  }, [projectId]);

  return null;
}

export function RendererRecreationPacketCollector({
  windowMode,
}: {
  windowMode: WorkbenchWindowMode;
}) {
  const tabs = useWorkbenchStore((state) => state.tabs);
  const activeTabId = useWorkbenchStore((state) => state.activeTabId);
  const harnesses = useWorkbenchStore((state) => state.harnesses);
  const selectedHarnessId = useWorkbenchStore(
    (state) => state.selectedHarnessId,
  );
  const activeTab = tabs.find((tab) => tab.id === activeTabId);
  const admitted = admitsFocusedCodexRendererRecreation({
    windowMode,
    activeTabId,
    focusedChatId: activeTab?.kind === "chat" ? activeTab.id : null,
    harness: harnesses.find((harness) => harness.id === selectedHarnessId),
  });

  useEffect(() => {
    if (!admitted) return;
    return mountRendererRecreationPacketCollector();
  }, [admitted]);

  return null;
}
