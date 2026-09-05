import {
  Eye,
  Files,
  GitCompareArrows,
  PanelRightClose,
  PanelRightOpen,
  Pin,
  X,
} from "lucide-react";
import { useEffect, useId, useLayoutEffect, useMemo, useRef } from "react";
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
import { agentArtifactVisible } from "./agentArtifactAdmission";
import { FileKindIcon } from "../features/files/FileKindIcon";
import { HarnessBrowserSurface } from "../features/preview/HarnessBrowserSurface";
import { useHarnessBrowseSession } from "../features/preview/useHarnessBrowseSession";
import { artifactChipStatsForTab } from "./artifactChipStats";
import { renderFileOrDiffCanvas } from "./surfaceRegistry";
import {
  agentArtifactOwnerKey,
  type AgentArtifactOwner,
} from "../domain/agentArtifact";

function artifactTabParts(tab: {
  resource?: { relativePath?: string };
  title: string;
}): [string, string, string] {
  const full = tab.resource?.relativePath ?? tab.title;
  const base = full.split("/").at(-1) ?? full;
  return [full, base, full.slice(0, full.length - base.length)];
}

export function AgentArtifactPane({
  expanded = true,
  focusOnExpand = false,
  onExpandedChange,
}: {
  expanded?: boolean;
  focusOnExpand?: boolean;
  onExpandedChange?: (expanded: boolean) => void;
} = {}) {
  const tabs = useWorkbenchStore((state) => state.tabs);
  const activeTabId = useWorkbenchStore((state) => state.activeTabId);
  const activateTab = useWorkbenchStore((state) => state.activateTab);
  const closeTab = useWorkbenchStore((state) => state.closeTab);
  const availableResourceKeys = useRef(new Set<string>());
  const paneRef = useRef<HTMLElement | null>(null);
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
  const browseSession = useHarnessBrowseSession(focusedOwner);
  const browseRevision = browseSession?.revision ?? 0;
  const previewOpen = browseSession !== null;
  const availableTargets = useMemo<AgentArtifactTarget[]>(
    () => [
      ...artifacts.map((tab) => ({ kind: "tab" as const, tabId: tab.id })),
      ...(focusedOwner !== null ? [{ kind: "preview" as const }] : []),
    ],
    [artifacts, focusedOwner],
  );
  const followRecord = agentArtifactFollowRecordFor(
    followRecords,
    focusedOwner,
  );
  const activeWorkbenchArtifact = artifacts.find(
    (tab) => tab.id === activeTabId,
  );
  const defaultTarget: AgentArtifactTarget | null = activeWorkbenchArtifact
    ? { kind: "tab", tabId: activeWorkbenchArtifact.id }
    : previewOpen
      ? { kind: "preview" }
      : null;
  const selectedTarget = followRecord
    ? (availableTargets.find((target) =>
        sameAgentArtifactTarget(target, followRecord.selected),
      ) ?? null)
    : defaultTarget;
  const previewSelected = selectedTarget?.kind === "preview";
  const activeArtifact =
    selectedTarget?.kind === "tab"
      ? (artifacts.find((tab) => tab.id === selectedTarget.tabId) ?? null)
      : null;

  useEffect(() => {
    if (!focusedOwner || !browseSession) return;
    useAgentArtifactFollowStore
      .getState()
      .observe(focusedOwner, { kind: "preview" }, browseSession.revision);
  }, [browseSession, focusedOwner]);

  useEffect(() => {
    if (!focusedOwner) return;
    useAgentArtifactFollowStore
      .getState()
      .reconcile(focusedOwner, availableTargets);
  }, [availableTargets, focusedOwner]);

  const hasResources = artifacts.length > 0 || focusedOwner !== null;
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
    const collapsedCount = artifacts.length;
    const collapsedLabel =
      collapsedCount > 0
        ? `Open files, changes, and preview, ${collapsedCount} ${collapsedCount === 1 ? "resource" : "resources"}`
        : "Open files, changes, and preview";
    return (
      <aside
        ref={paneRef}
        className="agent-artifact-pane agent-artifact-pane--collapsed"
        aria-label="Agent artifacts"
      >
        <button
          ref={edgeTriggerRef}
          className="agent-artifact-pane__edge-action"
          type="button"
          aria-label={collapsedLabel}
          title={
            collapsedCount > 0
              ? `Open resources (${collapsedCount})`
              : "Open resources"
          }
          onClick={() => onExpandedChange?.(true)}
        >
          <PanelRightOpen size={17} strokeWidth={1.8} aria-hidden="true" />
          {collapsedCount > 0 ? (
            <span className="agent-artifact-pane__count" aria-hidden="true">
              {collapsedCount}
            </span>
          ) : null}
        </button>
      </aside>
    );
  }

  return (
    <aside
      ref={paneRef}
      className={`agent-artifact-pane${
        hasResources ? "" : " agent-artifact-pane--empty"
      }`}
      aria-label="Agent artifacts"
      tabIndex={focusOnExpand ? -1 : undefined}
      onKeyDown={(event) => {
        if (event.key !== "Escape") return;
        event.preventDefault();
        onExpandedChange?.(false);
      }}
    >
      <header className="agent-artifact-pane__header">
        {hasResources ? (
          <nav className="tab-strip" aria-label="Agent artifact tabs">
            <div className="tab-strip__scroll">
              {artifacts.map((tab) => {
                const selected =
                  selectedTarget?.kind === "tab" &&
                  tab.id === selectedTarget.tabId;
                const stats = artifactChipStatsForTab(
                  tab,
                  focusedOwner ? focusedPage : null,
                );
                const [fullPath, basename, directory] = artifactTabParts(tab);
                const accessibleName = `${tab.kind === "file" ? "File" : "Diff"} ${fullPath}`;
                return (
                  <div
                    className={`tab ${selected ? "tab--active" : ""}`}
                    key={tab.id}
                    role="presentation"
                  >
                    <button
                      className="tab__activate"
                      type="button"
                      role="tab"
                      aria-selected={selected}
                      aria-label={accessibleName}
                      title={fullPath}
                      onClick={() => {
                        if (focusedOwner) {
                          useAgentArtifactFollowStore
                            .getState()
                            .pin(focusedOwner, {
                              kind: "tab",
                              tabId: tab.id,
                            });
                        }
                        void activateTab(tab.id);
                      }}
                    >
                      {tab.kind === "file" ? (
                        <FileKindIcon
                          path={tab.resource?.relativePath ?? tab.title}
                          size={15}
                        />
                      ) : (
                        <GitCompareArrows
                          size={15}
                          strokeWidth={1.8}
                          aria-hidden="true"
                        />
                      )}
                      <span className="tab__path" aria-hidden="true">
                        <span className="tab__dir">{directory}</span>
                        <span className="tab__basename">{basename}</span>
                      </span>
                      {tab.dirty === true ? (
                        <span className="tab__dirty" title="Unsaved changes" />
                      ) : null}
                      {stats?.additionsLabel ? (
                        <span className="tab__count tab__count--add">
                          {stats.additionsLabel}
                        </span>
                      ) : null}
                      {stats?.deletionsLabel ? (
                        <span className="tab__count tab__count--del">
                          {stats.deletionsLabel}
                        </span>
                      ) : null}
                    </button>
                    <button
                      className="tab__close"
                      type="button"
                      aria-label={`Close ${tab.title}`}
                      onClick={() => closeTab(tab.id)}
                    >
                      <X size={14} aria-hidden="true" />
                    </button>
                  </div>
                );
              })}
              {focusedOwner !== null ? (
                <div
                  className={`tab ${previewSelected ? "tab--active" : ""}`}
                  role="presentation"
                >
                  <button
                    className="tab__activate"
                    type="button"
                    role="tab"
                    aria-selected={previewSelected}
                    aria-label="Preview"
                    onClick={() => {
                      if (focusedOwner) {
                        useAgentArtifactFollowStore
                          .getState()
                          .pin(focusedOwner, { kind: "preview" });
                      }
                    }}
                  >
                    <Eye size={15} strokeWidth={1.8} aria-hidden="true" />
                    <span>Preview</span>
                  </button>
                </div>
              ) : null}
            </div>
          </nav>
        ) : (
          <strong>Files, changes, and preview</strong>
        )}
        {focusedOwner && followRecord && availableTargets.length > 1 ? (
          <button
            className="agent-artifact-pane__follow"
            type="button"
            aria-label="Follow latest resource"
            aria-pressed={followRecord.mode === "follow"}
            title={
              followRecord.mode === "follow"
                ? "Following the newest resource from this Chat"
                : "Resume following the newest resource from this Chat"
            }
            onClick={() =>
              useAgentArtifactFollowStore.getState().followLatest(focusedOwner)
            }
          >
            <Pin size={15} aria-hidden="true" />
          </button>
        ) : null}
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
      <div className="agent-artifact-pane__canvas">
        {previewSelected ? (
          <div className="agent-preview-stack">
            <HarnessBrowserSurface owner={focusedOwner} />
          </div>
        ) : null}
        {!previewSelected && activeArtifact
          ? renderFileOrDiffCanvas(activeArtifact)
          : null}
        {!previewSelected && !activeArtifact ? (
          <section
            className="placeholder-surface agent-artifact-pane__empty-state"
            aria-labelledby={emptyTitleId}
          >
            <span className="placeholder-surface__icon" aria-hidden="true">
              <Files size={19} strokeWidth={1.7} />
            </span>
            <h1 id={emptyTitleId}>No resources yet</h1>
            <p>Files, changes, and previews from this Chat will appear here.</p>
          </section>
        ) : null}
      </div>
    </aside>
  );
}
