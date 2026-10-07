import { ExternalLink, Files, PanelRightClose, Plus } from "lucide-react";
import {
  useEffect,
  useId,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
} from "react";
import { useStore } from "zustand";

import { useAgentArtifactAdmissionStore } from "../app/agentArtifactAdmissionStore";
import {
  agentArtifactFollowRecordFor,
  sameAgentArtifactTarget,
  type AgentArtifactTarget,
  useAgentArtifactFollowStore,
} from "../app/agentArtifactFollowStore";
import { acpProjectionStoreFor } from "../app/acpProjectionRegistry";
import { usePinnedAgentChatOwner } from "../app/agentChatOwner";
import { orderedTabs } from "../app/tabState";
import { useWorkbenchStore } from "../app/store";
import {
  agentArtifactAdmitted,
  agentArtifactVisible,
} from "./agentArtifactAdmission";
import { AgentArtifactCollapsedPane } from "./AgentArtifactCollapsedPane";
import { artifactChipStatsForTab } from "./artifactChipStats";
import { activeArtifactForTarget } from "./agentArtifactTabPresentation";
import { AgentArtifactHeader } from "./AgentArtifactHeader";
import { AgentArtifactTabStrip } from "./AgentArtifactTabStrip";
import { agentPreviewControllerOwner } from "./agentPreviewControllerOwner";
import { useAgentArtifactFollowLifecycle } from "./useAgentArtifactFollowLifecycle";
import { AgentPreviewSurface } from "../features/preview/AgentPreviewSurface";
import {
  useHarnessBrowseOwner,
  useHarnessBrowseSession,
} from "../features/preview/useHarnessBrowseSession";
import {
  renderFileOrDiffCanvas,
  renderRegisteredCanvas,
} from "./surfaceRegistry";
import {
  agentArtifactOwnerKey,
  type AgentArtifactOwner,
} from "../domain/agentArtifact";

export function AgentArtifactPane({
  expanded = true,
  focusOnExpand = false,
  onExpandedChange,
  onOpenInEditor,
}: {
  expanded?: boolean;
  focusOnExpand?: boolean;
  onExpandedChange?: (expanded: boolean) => void;
  onOpenInEditor?: (tabId: string) => void;
} = {}) {
  const tabs = useWorkbenchStore((state) => state.tabs);
  const activeTabId = useWorkbenchStore((state) => state.activeTabId);
  const activateTab = useWorkbenchStore((state) => state.activateTab);
  const closeTab = useWorkbenchStore((state) => state.closeTab);
  const openBrowser = useWorkbenchStore((state) => state.openBrowser);
  const availableResourceKeys = useRef(new Set<string>());
  const paneRef = useRef<HTMLElement | null>(null);
  const [canvasNode, setCanvasNode] = useState<HTMLElement | null>(null);
  const edgeTriggerRef = useRef<HTMLButtonElement | null>(null);
  const previousExpanded = useRef(expanded);
  const emptyTitleId = useId();
  const admissions = useAgentArtifactAdmissionStore(
    (state) => state.admissions,
  );
  const followRecords = useAgentArtifactFollowStore((state) => state.records);
  const project = useWorkbenchStore((state) => state.project);
  const projectGeneration = useWorkbenchStore(
    (state) => state.projectGeneration,
  );
  const nativeTransitionEpoch = useWorkbenchStore(
    (state) => state.nativeTransitionEpoch,
  );
  const projectTransitioning = useWorkbenchStore(
    (state) => state.projectTransitioning,
  );
  const focusedChatId = usePinnedAgentChatOwner();
  const focusedProjectionStore = acpProjectionStoreFor(focusedChatId);
  const focusedPage = useStore(focusedProjectionStore, (state) => state.page);
  const focusedScope = useStore(focusedProjectionStore, (state) => state.scope);
  const focusedOwner = useMemo<AgentArtifactOwner | null>(() => {
    if (
      projectTransitioning ||
      !project ||
      !focusedChatId ||
      !focusedPage?.currentSessionId ||
      !focusedScope ||
      focusedPage.projectId !== project.id ||
      focusedPage.chatId !== focusedChatId ||
      focusedScope.projectId !== focusedPage.projectId ||
      focusedScope.chatId !== focusedPage.chatId ||
      focusedScope.projectGeneration !== String(projectGeneration)
    ) {
      return null;
    }
    return {
      projectId: focusedPage.projectId,
      chatId: focusedPage.chatId,
      sessionId: focusedPage.currentSessionId,
    };
  }, [
    focusedChatId,
    focusedPage,
    focusedScope,
    project,
    projectGeneration,
    projectTransitioning,
  ]);
  const artifacts = useMemo(
    () =>
      orderedTabs(tabs).filter((tab) =>
        agentArtifactVisible({
          tabId: tab.id,
          kind: tab.kind,
          admissions,
          focusedOwner,
          hasFocusedChatStats:
            artifactChipStatsForTab(tab, focusedOwner ? focusedPage : null) !=
            null,
        }),
      ),
    [admissions, focusedOwner, focusedPage, tabs],
  );
  const browseOwner = useHarnessBrowseOwner();
  const browseSession = useHarnessBrowseSession(browseOwner);
  const browseRevision = browseSession?.revision ?? 0;
  const previewOpen = browseSession !== null;
  const availableTargets = useMemo<AgentArtifactTarget[]>(
    () => [
      ...artifacts.map((tab) => ({ kind: "tab" as const, tabId: tab.id })),
      ...(focusedOwner !== null && previewOpen
        ? [{ kind: "preview" as const }]
        : []),
    ],
    [artifacts, focusedOwner, previewOpen],
  );
  const record = agentArtifactFollowRecordFor(followRecords, focusedOwner);
  const activeWorkbenchArtifact = artifacts.find(
    ({ id }) => id === activeTabId,
  );
  const latestArtifact = artifacts.at(-1);
  const defaultTarget: AgentArtifactTarget | null = activeWorkbenchArtifact
    ? { kind: "tab", tabId: activeWorkbenchArtifact.id }
    : previewOpen
      ? { kind: "preview" }
      : latestArtifact
        ? { kind: "tab", tabId: latestArtifact.id }
        : null;
  const selectedTarget = record
    ? (availableTargets.find((target) =>
        sameAgentArtifactTarget(target, record.selected),
      ) ?? null)
    : defaultTarget;
  const previewSelected = selectedTarget?.kind === "preview";
  const previewControllerOwner = agentPreviewControllerOwner({
    browseOwner,
    focusedOwner,
    focusedPage,
    focusedScope,
    nativeTransitionEpoch,
    projectGeneration,
  });
  const previewController = (
    <AgentPreviewSurface
      key={JSON.stringify(Object.values(previewControllerOwner ?? {}))}
      owner={previewControllerOwner}
      browseOwner={browseOwner}
      visible={expanded && previewSelected}
      container={canvasNode}
    />
  );
  const activeArtifact = activeArtifactForTarget(artifacts, selectedTarget);
  const activeArtifactAdmitted = Boolean(
    activeArtifact &&
    agentArtifactAdmitted(admissions, focusedOwner, activeArtifact.id),
  );
  const selectedPosition = availableTargets.findIndex((target) =>
    sameAgentArtifactTarget(target, selectedTarget),
  );
  const stepArtifact = (direction: -1 | 1) => {
    if (!focusedOwner || selectedPosition < 0) return;
    const target = availableTargets[selectedPosition + direction];
    if (!target) return;
    useAgentArtifactFollowStore.getState().pin(focusedOwner, target);
    if (target.kind === "tab") void activateTab(target.tabId);
  };
  const focusedRuntimeEpoch =
    focusedPage?.sessions.find(
      (session) => session.id === focusedOwner?.sessionId,
    )?.runtimeEpoch ?? null;

  useAgentArtifactFollowLifecycle({
    availableTargets,
    browseSession,
    focusedOwner,
  });

  const hasResources = artifacts.length > 0 || previewOpen;
  const resourceKeys = useMemo(
    () =>
      new Set([
        ...artifacts.map((tab) => tab.id),
        ...(previewOpen
          ? [`preview:${agentArtifactOwnerKey(focusedOwner)}:${browseRevision}`]
          : []),
      ]),
    [artifacts, browseRevision, focusedOwner, previewOpen],
  );

  useEffect(() => {
    const added = [...resourceKeys].some(
      (resourceKey) => !availableResourceKeys.current.has(resourceKey),
    );
    availableResourceKeys.current = resourceKeys;
    if (!expanded && added) onExpandedChange?.(true);
  }, [expanded, onExpandedChange, resourceKeys]);

  useLayoutEffect(() => {
    if (expanded && !previousExpanded.current && focusOnExpand) {
      paneRef.current?.focus();
    }
    if (!expanded && previousExpanded.current) {
      edgeTriggerRef.current?.focus();
    }
    previousExpanded.current = expanded;
  }, [expanded, focusOnExpand]);

  if (!expanded) {
    return (
      <>
        <AgentArtifactCollapsedPane
          count={artifacts.length + (previewOpen ? 1 : 0)}
          edgeTriggerRef={edgeTriggerRef}
          onExpand={() => onExpandedChange?.(true)}
          paneRef={paneRef}
        />
        {previewController}
      </>
    );
  }

  return (
    <>
      <aside
        ref={paneRef}
        className={`agent-artifact-pane${
          hasResources ? "" : " agent-artifact-pane--empty"
        }${activeArtifact && activeArtifact.kind !== "browser" ? " agent-artifact-pane--with-header" : ""}`}
        aria-label="Agent artifacts"
        data-owner-project-id={focusedOwner?.projectId}
        data-owner-chat-id={focusedOwner?.chatId}
        data-owner-session-id={focusedOwner?.sessionId}
        data-owner-runtime-epoch={focusedRuntimeEpoch ?? undefined}
        tabIndex={focusOnExpand ? -1 : undefined}
        onKeyDown={(event) => {
          if (event.key !== "Escape") return;
          event.preventDefault();
          onExpandedChange?.(false);
        }}
      >
        <header className="agent-artifact-pane__header">
          {hasResources ? (
            <AgentArtifactTabStrip
              artifacts={artifacts}
              selectedTarget={selectedTarget}
              focusedOwner={focusedOwner}
              focusedPage={focusedPage}
              previewSelected={previewSelected}
              previewOpen={previewOpen}
              record={record}
              availableTargets={availableTargets}
              activateTab={activateTab}
              closeTab={closeTab}
            />
          ) : (
            <strong>Files, changes, and preview</strong>
          )}
          <button
            className="agent-artifact-pane__collapse"
            type="button"
            aria-label="New browser tab"
            title="New browser tab"
            disabled={!project}
            onClick={() => void openBrowser()}
          >
            <Plus size={16} aria-hidden="true" />
          </button>
          <button
            className="agent-artifact-pane__collapse"
            type="button"
            aria-label="Pop out active tab"
            title="Open active tab in Editor"
            disabled={
              !activeArtifact ||
              activeArtifact.kind === "browser" ||
              !onOpenInEditor
            }
            onClick={() => {
              if (activeArtifact && activeArtifact.kind !== "browser")
                onOpenInEditor?.(activeArtifact.id);
            }}
          >
            <ExternalLink size={16} aria-hidden="true" />
          </button>
          <button
            className="agent-artifact-pane__collapse"
            type="button"
            aria-label="Collapse files, changes, and preview"
            title="Collapse resources"
            onClick={() => onExpandedChange?.(false)}
          >
            <PanelRightClose size={16} aria-hidden="true" />
          </button>
        </header>
        {activeArtifact && activeArtifact.kind !== "browser" ? (
          <AgentArtifactHeader
            key={activeArtifact.id}
            tab={activeArtifact}
            admitted={activeArtifactAdmitted}
            page={focusedOwner ? focusedPage : null}
            position={selectedPosition}
            total={availableTargets.length}
            onStep={stepArtifact}
            onOpenInEditor={() => onOpenInEditor?.(activeArtifact.id)}
          />
        ) : null}
        <div ref={setCanvasNode} className="agent-artifact-pane__canvas">
          {!previewSelected && activeArtifact
            ? activeArtifact.kind === "browser"
              ? renderRegisteredCanvas("browser", activeArtifact)
              : renderFileOrDiffCanvas(activeArtifact, { readOnly: true })
            : null}
          {!previewSelected && !activeArtifact ? (
            <section
              className="placeholder-surface agent-artifact-pane__empty-state"
              aria-labelledby={emptyTitleId}
            >
              <span className="placeholder-surface__icon" aria-hidden="true">
                <Files size={19} strokeWidth={1.7} />
              </span>
              <h1 id={emptyTitleId}>Waiting for this Chat</h1>
              <p>
                Open a file or change from the conversation. Preview will appear
                when this Chat starts a loopback server.
              </p>
            </section>
          ) : null}
        </div>
      </aside>
      {previewController}
    </>
  );
}
