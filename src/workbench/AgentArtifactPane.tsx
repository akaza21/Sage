import { lazy, Suspense, useId, useMemo, useRef, useState } from "react";
import { useStore } from "zustand";
import type { PreviewTargetRef } from "../domain/previewProjection";

import { acpProjectionStoreFor } from "../app/acpProjectionRegistry";
import { useAgentArtifactAdmissionStore } from "../app/agentArtifactAdmissionStore";
import {
  agentArtifactFollowRecordFor,
  sameAgentArtifactTarget,
  useAgentArtifactFollowStore,
  type AgentArtifactTarget,
} from "../app/agentArtifactFollowStore";
import { usePinnedAgentChatOwner } from "../app/agentChatOwner";
import { useWorkbenchStore } from "../app/store";
import {
  latestSubagentForPage,
  ownerFromFocusedPage,
  subagentPaneForPage,
  useSubagentPaneStore,
  validFocusedSubagentPage,
} from "../app/subagentPaneStore";
import { orderedTabs } from "../app/tabState";
import type { AgentArtifactOwner } from "../domain/agentArtifact";
import { AgentPreviewSurface } from "../features/preview/AgentPreviewSurface";
import { RetainedActivity } from "../ui/RetainedActivity";
import { useRetainedElement } from "../ui/useRetainedElement";
import {
  agentArtifactAdmitted,
  agentArtifactVisible,
} from "./agentArtifactAdmission";
import { AgentArtifactCollapsedPane } from "./AgentArtifactCollapsedPane";
import { AgentArtifactFocusGuard } from "./AgentArtifactFocusGuard";
import {
  AgentBrowserPaneControls,
  AgentBrowserPaneStatus,
} from "./AgentArtifactPaneControls";
import { lazyParts } from "../features/chat/lazyParts";
import { activeArtifactForTarget } from "./agentArtifactTabPresentation";
import { useAgentChangesState } from "./agentChangesState";
import { agentPreviewControllerOwner } from "./agentPreviewControllerOwner";
import { artifactChipStatsForTab } from "./artifactChipStats";
import { loadSubagentSurface } from "./loadSubagentSurface";
import {
  renderFileOrDiffCanvas,
  renderRegisteredCanvas,
} from "./surfaceRegistry";
import {
  useAgentArtifactFollowLifecycle,
  useExpandOnNewArtifact,
  useFocusedBrowse,
  useSelectOpenedBrowserTab,
} from "./useAgentArtifactFollowLifecycle";
import { useAgentArtifactPaneFocus } from "./useAgentArtifactPaneFocus";
import { useAgentChangesPane, useChangesOnExpand } from "./useAgentChangesPane";
import { useSubagentPaneExpansion } from "./useSubagentPaneExpansion";

const SubagentSurface = lazy(() =>
  loadSubagentSurface().then((module) => ({
    default: module.SubagentSurface,
  })),
);
const TabsPart = lazyParts(() => import("./AgentArtifactTabStrip"));
const AgentArtifactHeader = lazy(() =>
  import("./AgentArtifactHeader").then((module) => ({
    default: module.AgentArtifactHeader,
  })),
);

const PANE_CANVAS = { readOnly: true, hostHeader: true };

export function AgentArtifactPane({
  expanded = true,
  focusOnExpand = false,
  modal = false,
  overlayOnExpand = focusOnExpand,
  onExpandedChange,
  onOpenInEditor,
  headerNode,
  onMaximizedChange,
  onPreviewTargetChange,
  maximized: suppliedMaximized,
}: {
  expanded?: boolean;
  focusOnExpand?: boolean;
  modal?: boolean;
  overlayOnExpand?: boolean;
  onExpandedChange?: (expanded: boolean) => void;
  onOpenInEditor?: (tabId: string) => void;
  headerNode?: HTMLElement | null;
  onMaximizedChange?: (maximized: boolean) => void;
  onPreviewTargetChange?: (target: PreviewTargetRef | null) => void;
  maximized?: boolean;
} = {}) {
  const tabs = useWorkbenchStore((state) => state.tabs);
  const activeTabId = useWorkbenchStore((state) => state.activeTabId);
  const userOpenedBrowserTabId = useWorkbenchStore(
    (state) => state.userOpenedBrowserTabId,
  );
  const activateTab = useWorkbenchStore((state) => state.activateTab);
  const closeTab = useWorkbenchStore((state) => state.closeTab);
  const paneRef = useRef<HTMLElement | null>(null);
  const [canvasNode, setCanvasNode] = useRetainedElement<HTMLDivElement>();
  const [localMaximized, setLocalMaximized] = useState(false);
  const maximized = suppliedMaximized ?? localMaximized;
  const setMaximized = (next: boolean) => {
    setLocalMaximized(next);
    onMaximizedChange?.(next);
  };
  const edgeTriggerRef = useRef<HTMLButtonElement | null>(null);
  const onPaneKeyDown = useAgentArtifactPaneFocus({
    expanded,
    overlay: focusOnExpand,
    overlayOnExpand,
    modal,
    paneRef,
    triggerRef: edgeTriggerRef,
    onExpandedChange,
  });
  const emptyTitleId = useId();
  const paneId = useId();
  const admissions = useAgentArtifactAdmissionStore(
    (state) => state.admissions,
  );
  const followRecords = useAgentArtifactFollowStore((state) => state.records);
  const subagentOpen = useSubagentPaneStore((state) => state.open);
  const subagentSelectedState = useSubagentPaneStore((state) => state.selected);
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
  const focusedPageValid = validFocusedSubagentPage(
    focusedPage,
    focusedScope,
    project?.id ?? null,
    focusedChatId,
    projectGeneration,
    projectTransitioning,
  );
  const focusedOwner = useMemo<AgentArtifactOwner | null>(
    () => ownerFromFocusedPage(focusedPage, focusedPageValid),
    [focusedPage, focusedPageValid],
  );
  const selectedSubagent = subagentPaneForPage(
    subagentOpen,
    focusedPageValid ? focusedPage : null,
  );
  const subagentTarget =
    selectedSubagent ??
    latestSubagentForPage(focusedPageValid ? focusedPage : null);
  const subagentAvailable = subagentTarget !== null;
  const changes = useAgentChangesPane(
    focusedOwner,
    projectGeneration,
    expanded,
    onExpandedChange,
    projectTransitioning ? null : (project?.id ?? null),
    focusedChatId,
    focusedPageValid && focusedPage?.currentSessionId === null,
  );
  const subagentSelected =
    !changes.selected && selectedSubagent !== null && subagentSelectedState;
  useSubagentPaneExpansion({
    expanded,
    selected: subagentSelected,
    target: subagentOpen,
    onExpandedChange,
  });
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
  const { browseOwner, browseSession } = useFocusedBrowse(focusedOwner);
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
  const previewSelected =
    !changes.selected &&
    !subagentSelected &&
    selectedTarget?.kind === "preview";
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
      onTargetChange={onPreviewTargetChange}
      key={JSON.stringify(Object.values(previewControllerOwner ?? {}))}
      owner={previewControllerOwner}
      browseOwner={browseOwner}
      selected={previewSelected}
      visible={expanded && previewSelected}
      container={canvasNode}
    />
  );
  const activeArtifact =
    changes.selected || subagentSelected
      ? null
      : activeArtifactForTarget(artifacts, selectedTarget);
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
    browseOwnerChatId: browseOwner?.chatId ?? null,
    focusedOwner,
  });
  useSelectOpenedBrowserTab({
    artifacts,
    activeTabId,
    userOpenedBrowserTabId,
    focusedOwner,
  });

  const hasResources =
    changes.open || artifacts.length > 0 || previewOpen || subagentAvailable;
  useChangesOnExpand(expanded, !hasResources, changes.owner, projectGeneration);
  useExpandOnNewArtifact({
    artifacts,
    previewOpen,
    browseRevision: browseSession?.revision ?? 0,
    focusedOwner,
    expanded,
    onExpandedChange,
  });

  return (
    <div className="agent-artifact-surface" onKeyDown={onPaneKeyDown}>
      {expanded ? null : (
        <AgentArtifactCollapsedPane
          count={
            artifacts.length + (previewOpen ? 1 : 0) + (changes.open ? 1 : 0)
          }
          subagentPage={subagentAvailable ? focusedPage : null}
          subagentItemId={subagentTarget?.itemId}
          edgeTriggerRef={edgeTriggerRef}
          onExpand={() => onExpandedChange?.(true)}
          paneId={paneId}
          overlay={overlayOnExpand}
          headerNode={headerNode}
        />
      )}
      <RetainedActivity visible={expanded}>
        <aside
          id={paneId}
          ref={paneRef}
          role={focusOnExpand ? "dialog" : undefined}
          aria-modal={modal || undefined}
          className={`agent-artifact-pane surface surface--card${
            hasResources ? "" : " agent-artifact-pane--empty"
          }${activeArtifact && activeArtifact.kind !== "browser" ? " agent-artifact-pane--with-header" : ""}${maximized ? " agent-artifact-pane--maximized" : ""}`}
          aria-label="Agent artifacts"
          data-owner-project-id={focusedOwner?.projectId}
          data-owner-chat-id={focusedOwner?.chatId}
          data-owner-session-id={focusedOwner?.sessionId}
          data-owner-runtime-epoch={focusedRuntimeEpoch ?? undefined}
          tabIndex={focusOnExpand ? -1 : undefined}
        >
          <AgentArtifactFocusGuard active={focusOnExpand} paneRef={paneRef} />
          <header className="agent-artifact-pane__header">
            {hasResources ? (
              <TabsPart
                part="AgentArtifactTabStrip"
                artifacts={artifacts}
                selectedTarget={
                  changes.selected || subagentSelected ? null : selectedTarget
                }
                changesTab={changes.tab}
                focusedOwner={focusedOwner}
                focusedPage={focusedPage}
                previewSelected={previewSelected}
                previewOpen={previewOpen}
                subagentOpen={subagentAvailable}
                subagentSelected={subagentSelected}
                onSelectSubagent={() => {
                  useAgentChangesState.getState().select(false);
                  if (subagentTarget)
                    useSubagentPaneStore.getState().show(subagentTarget, true);
                }}
                record={record}
                availableTargets={availableTargets}
                activateTab={activateTab}
                closeTab={closeTab}
              />
            ) : (
              <strong>Files, changes, and preview</strong>
            )}
            <AgentBrowserPaneControls
              maximized={maximized}
              onMaximizedChange={setMaximized}
              onCollapse={() => {
                setMaximized(false);
                onExpandedChange?.(false);
              }}
            />
          </header>
          {activeArtifact && activeArtifact.kind !== "browser" ? (
            <Suspense fallback={null}>
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
            </Suspense>
          ) : null}
          <div ref={setCanvasNode} className="agent-artifact-pane__canvas">
            <AgentBrowserPaneStatus />
            {changes.surface}
            {subagentSelected && focusedPage && subagentTarget ? (
              <Suspense fallback={<p role="status">Loading subagent view…</p>}>
                <SubagentSurface
                  page={focusedPage}
                  projectGeneration={String(nativeTransitionEpoch)}
                  itemId={subagentTarget.itemId}
                  agentKey={subagentTarget.agentKey}
                />
              </Suspense>
            ) : null}
            {!previewSelected && activeArtifact
              ? activeArtifact.kind === "browser"
                ? renderRegisteredCanvas("browser", activeArtifact)
                : renderFileOrDiffCanvas(activeArtifact, PANE_CANVAS)
              : null}
            {!changes.selected &&
            !subagentSelected &&
            !previewSelected &&
            !activeArtifact ? (
              <TabsPart part="AgentArtifactEmptyState" titleId={emptyTitleId} />
            ) : null}
          </div>
          <AgentArtifactFocusGuard active={focusOnExpand} paneRef={paneRef} />
        </aside>
      </RetainedActivity>
      {previewController}
    </div>
  );
}
