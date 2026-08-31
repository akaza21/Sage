import { Eye, Files, GitCompareArrows, PanelRightClose, X } from "lucide-react";
import { useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import { useStore } from "zustand";

import { useAgentArtifactAdmissionStore } from "../app/agentArtifactAdmissionStore";
import { acpProjectionStoreFor } from "../app/acpProjectionRegistry";
import { orderedTabs } from "../app/tabState";
import { useWorkbenchStore } from "../app/store";
import { agentArtifactVisible } from "./agentArtifactAdmission";
import { FileKindIcon } from "../features/files/FileKindIcon";
import { HarnessBrowserSurface } from "../features/preview/HarnessBrowserSurface";
import {
  useHarnessBrowseOwner,
  useHarnessBrowseSession,
} from "../features/preview/useHarnessBrowseSession";
import { artifactChipStatsForTab } from "./artifactChipStats";
import { renderFileOrDiffCanvas } from "./surfaceRegistry";

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
  const [previewSelected, setPreviewSelected] = useState(false);
  const lastArtifactId = useRef(activeTabId);
  const lastBrowseRevision = useRef(0);
  const availableResourceKeys = useRef(new Set<string>());
  const paneRef = useRef<HTMLElement | null>(null);
  const edgeTriggerRef = useRef<HTMLButtonElement | null>(null);
  const previousExpanded = useRef(expanded);
  const admittedIds = useAgentArtifactAdmissionStore((state) => state.ids);
  const browseChatId = useHarnessBrowseOwner();
  const focusedPage = useStore(acpProjectionStoreFor(browseChatId), (state) =>
    browseChatId && state.page?.chatId === browseChatId ? state.page : null,
  );
  const artifacts = useMemo(
    () =>
      orderedTabs(tabs).filter((tab) =>
        agentArtifactVisible({
          tabId: tab.id,
          kind: tab.kind,
          admittedIds,
          hasFocusedChatStats:
            artifactChipStatsForTab(tab, focusedPage) != null,
        }),
      ),
    [admittedIds, focusedPage, tabs],
  );
  const activeArtifact =
    artifacts.find((tab) => tab.id === activeTabId) ?? null;
  const browseSession = useHarnessBrowseSession(browseChatId);
  const browseRevision = browseSession?.revision ?? 0;
  const previewOpen = browseSession !== null;

  useEffect(() => {
    if (activeArtifact && activeTabId !== lastArtifactId.current) {
      setPreviewSelected(false);
    }
    lastArtifactId.current = activeTabId;
  }, [activeArtifact, activeTabId]);

  useEffect(() => {
    if (browseRevision > lastBrowseRevision.current) {
      setPreviewSelected(true);
    }
    lastBrowseRevision.current = browseRevision;
  }, [browseRevision]);

  const hasResources = artifacts.length > 0 || previewOpen;
  const resourceKeys = useMemo(
    () =>
      new Set([
        ...artifacts.map((tab) => tab.id),
        ...(previewOpen ? [`preview:${browseChatId}:${browseRevision}`] : []),
      ]),
    [artifacts, browseChatId, browseRevision, previewOpen],
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
      <aside
        ref={paneRef}
        className="agent-artifact-pane agent-artifact-pane--collapsed"
        aria-label="Agent artifacts"
      >
        <button
          ref={edgeTriggerRef}
          className="agent-artifact-pane__edge-action"
          type="button"
          aria-label="Open files, changes, and preview"
          onClick={() => onExpandedChange?.(true)}
        >
          <span>
            <Files size={16} aria-hidden="true" />
            <small>Files</small>
          </span>
          <span>
            <GitCompareArrows size={16} aria-hidden="true" />
            <small>Changes</small>
          </span>
          <span>
            <Eye size={16} aria-hidden="true" />
            <small>Preview</small>
          </span>
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
                const selected = !previewSelected && tab.id === activeTabId;
                const stats = artifactChipStatsForTab(tab, focusedPage);
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
                      onClick={() => {
                        setPreviewSelected(false);
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
                      <span>{tab.title}</span>
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
              {previewOpen ? (
                <div
                  className={`tab ${previewSelected ? "tab--active" : ""}`}
                  role="presentation"
                >
                  <button
                    className="tab__activate"
                    type="button"
                    role="tab"
                    aria-selected={previewSelected}
                    onClick={() => setPreviewSelected(true)}
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
        {previewSelected && previewOpen ? (
          <div className="agent-preview-stack">
            <HarnessBrowserSurface chatId={browseChatId} />
          </div>
        ) : null}
        {!previewSelected && activeArtifact
          ? renderFileOrDiffCanvas(activeArtifact)
          : null}
        {!previewSelected && !activeArtifact ? (
          <section className="placeholder-surface">
            <h1>Nothing open</h1>
            <p>Files, diffs, and browser previews from this Chat open here.</p>
          </section>
        ) : null}
      </div>
    </aside>
  );
}
