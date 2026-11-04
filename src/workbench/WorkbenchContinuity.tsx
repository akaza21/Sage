import { useEffect, useRef } from "react";
import { useWorkbenchStore } from "../app/store";
import { admitsFocusedCodexRendererRecreation } from "../features/chat/acp/rendererRecreationCollectorAdmission";
import { mountRendererRecreationPacketCollector } from "../features/chat/acp/rendererRecreationPacket";
import "./harness-browser-mode.css";
import "./workbench-window-mode.css";
import {
  agentCenterChatAction,
  isAgentArtifactTabKind,
  lastArtifactTabId,
  type AgentCenterDecision,
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

function agentCenterAttemptKey(
  decision: AgentCenterDecision,
  projectId: string | null,
): string | null {
  const scope = projectId ?? "";
  if (decision.action === "create") return `create:${scope}`;
  return null;
}

export function AgentCenterKeeper({
  windowMode,
}: {
  windowMode: WorkbenchWindowMode;
}) {
  const activeTabId = useWorkbenchStore((state) => state.activeTabId);
  const projectTransitioning = useWorkbenchStore(
    (state) => state.projectTransitioning,
  );
  const projectGeneration = useWorkbenchStore(
    (state) => state.projectGeneration,
  );
  const attemptedRef = useRef<string | null>(null);

  useEffect(() => {
    if (windowMode !== "agent") {
      attemptedRef.current = null;
      return;
    }
    if (projectTransitioning) return;
    const state = useWorkbenchStore.getState();
    const decision = agentCenterChatAction(state.tabs, activeTabId);
    if (decision.action === "none") {
      attemptedRef.current = null;
      return;
    }
    const attemptKey = agentCenterAttemptKey(
      decision,
      state.project?.id ?? null,
    );
    if (attemptKey === null || attemptedRef.current === attemptKey) return;
    attemptedRef.current = attemptKey;
    void state.createChat();
  }, [windowMode, activeTabId, projectTransitioning, projectGeneration]);

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
