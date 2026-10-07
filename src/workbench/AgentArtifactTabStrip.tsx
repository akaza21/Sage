import { Eye, GitCompareArrows, Globe2, Pin, X } from "lucide-react";
import { useRef } from "react";

import {
  agentArtifactPendingTargets,
  sameAgentArtifactTarget,
  type AgentArtifactFollowRecord,
  type AgentArtifactTarget,
  useAgentArtifactFollowStore,
} from "../app/agentArtifactFollowStore";
import type { AcpChatPage } from "../domain/acpProjection";
import type { AgentArtifactOwner } from "../domain/agentArtifact";
import type { WorkbenchTab } from "../domain/workbench";
import { FileKindIcon } from "../features/files/FileKindIcon";
import { BrowserFavicon } from "../features/browser/browserAddress";
import { artifactChipStatsForTab } from "./artifactChipStats";
import { artifactTabParts } from "./agentArtifactTabPresentation";

type AgentArtifactTabStripProps = {
  artifacts: readonly WorkbenchTab[];
  selectedTarget: AgentArtifactTarget | null;
  focusedOwner: AgentArtifactOwner | null;
  focusedPage: AcpChatPage | null;
  previewSelected: boolean;
  previewOpen: boolean;
  record: AgentArtifactFollowRecord | null;
  availableTargets: readonly AgentArtifactTarget[];
  activateTab: (tabId: string) => Promise<void> | void;
  closeTab: (tabId: string) => void;
};

export function AgentArtifactTabStrip({
  artifacts,
  selectedTarget,
  focusedOwner,
  focusedPage,
  previewSelected,
  previewOpen,
  record,
  availableTargets,
  activateTab,
  closeTab,
}: AgentArtifactTabStripProps) {
  const scrollRef = useRef<HTMLDivElement | null>(null);
  const pendingTargets = agentArtifactPendingTargets(record);
  const latestPendingTarget = pendingTargets.at(-1) ?? null;
  const following = record?.mode !== "pinned";
  const previewPending = pendingTargets.some(
    (target) => target.kind === "preview",
  );
  const fileOrDiffPending = pendingTargets.some(
    (target) => target.kind === "tab",
  );
  const pendingLabel = pendingTargets.length > 0 ? "New" : null;
  const descriptionIdForTab = (index: number) =>
    `agent-artifact-pending-${index}`;
  const modeLabel = following ? "Following latest" : "Pinned";
  const followTitle = following
    ? "Following the newest resource from this Chat"
    : latestPendingTarget
      ? "A newer resource arrived. Select to follow it."
      : "Pinned to this resource. Select to follow the newest resource.";
  return (
    <>
      <nav className="tab-strip" aria-label="Agent artifact tabs">
        <div ref={scrollRef} className="tab-strip__scroll">
          {artifacts.map((tab, index) => {
            const selected =
              selectedTarget?.kind === "tab" && tab.id === selectedTarget.tabId;
            const stats = artifactChipStatsForTab(
              tab,
              focusedOwner ? focusedPage : null,
            );
            const [fullPath, basename, directory] = artifactTabParts(tab);
            const kindLabel =
              tab.kind === "file"
                ? "File"
                : tab.kind === "browser"
                  ? "Browser"
                  : "Diff";
            const accessibleName = `${kindLabel} ${fullPath}`;
            const pending = pendingTargets.some((target) =>
              sameAgentArtifactTarget(target, {
                kind: "tab",
                tabId: tab.id,
              }),
            );
            return (
              <div
                className={`tab ${selected ? "tab--active" : ""}${
                  pending ? " tab--pending" : ""
                }`}
                key={tab.id}
                role="presentation"
                data-resource-kind={tab.kind}
              >
                <button
                  className="tab__activate"
                  type="button"
                  role="tab"
                  aria-selected={selected}
                  aria-label={accessibleName}
                  aria-describedby={
                    pending ? descriptionIdForTab(index) : undefined
                  }
                  data-resource-root-id={tab.resource?.rootId}
                  data-resource-relative-path={tab.resource?.relativePath}
                  title={accessibleName}
                  onClick={() => {
                    if (focusedOwner) {
                      useAgentArtifactFollowStore.getState().pin(focusedOwner, {
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
                      size={16}
                    />
                  ) : tab.kind === "browser" ? (
                    <BrowserFavicon
                      key={tab.favicon ?? ""}
                      value={tab.favicon}
                      className="tab__favicon"
                      fallback={
                        <Globe2
                          size={16}
                          strokeWidth={1.8}
                          aria-hidden="true"
                        />
                      }
                    />
                  ) : (
                    <GitCompareArrows
                      size={16}
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
                  {pending ? (
                    <span
                      id={descriptionIdForTab(index)}
                      className="agent-artifact-tab__new"
                      title="New resource arrived while pinned"
                    >
                      <span className="sr-only">New resource</span>
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
          {focusedOwner !== null && previewOpen ? (
            <div
              className={`tab ${previewSelected ? "tab--active" : ""}${
                previewPending ? " tab--pending" : ""
              }`}
              role="presentation"
              data-resource-kind="preview"
            >
              <button
                className="tab__activate"
                type="button"
                role="tab"
                aria-selected={previewSelected}
                aria-label="Preview"
                title="Preview"
                aria-describedby={
                  previewPending ? "preview-new-resource" : undefined
                }
                onClick={() =>
                  useAgentArtifactFollowStore
                    .getState()
                    .pin(focusedOwner, { kind: "preview" })
                }
              >
                <Eye size={16} strokeWidth={1.8} aria-hidden="true" />
                <span className="tab__path" aria-hidden="true">
                  <span className="tab__basename">Preview</span>
                </span>
                {previewPending ? (
                  <span
                    id="preview-new-resource"
                    className="agent-artifact-tab__new"
                    title="New resource arrived while pinned"
                  >
                    <span className="sr-only">New resource</span>
                  </span>
                ) : null}
              </button>
            </div>
          ) : null}
        </div>
      </nav>
      {focusedOwner && record && availableTargets.length > 1 ? (
        <button
          className={`agent-artifact-pane__follow agent-artifact-pane__follow--${record.mode}${
            latestPendingTarget ? " agent-artifact-pane__follow--pending" : ""
          }`}
          type="button"
          aria-label={
            following
              ? "Following latest resource"
              : previewPending && fileOrDiffPending
                ? "Pinned resource, new File, Diff, and Preview available"
                : previewPending
                  ? "Pinned resource, new Preview available"
                  : fileOrDiffPending
                    ? "Pinned resource, new File or Diff available"
                    : "Pinned resource"
          }
          aria-pressed={following}
          title={followTitle}
          onClick={() => {
            useAgentArtifactFollowStore.getState().followLatest(focusedOwner);
            requestAnimationFrame(() => {
              scrollRef.current
                ?.querySelector<HTMLElement>(
                  '[role="tab"][aria-selected="true"]',
                )
                ?.scrollIntoView?.({ block: "nearest", inline: "nearest" });
            });
          }}
        >
          {following ? (
            <Eye size={14} strokeWidth={1.9} aria-hidden="true" />
          ) : (
            <Pin size={14} strokeWidth={1.9} aria-hidden="true" />
          )}
          <span>{modeLabel}</span>
          {pendingLabel ? (
            <span
              className="agent-artifact-pane__follow-new"
              aria-hidden="true"
            >
              {pendingLabel}
            </span>
          ) : null}
        </button>
      ) : null}
    </>
  );
}
