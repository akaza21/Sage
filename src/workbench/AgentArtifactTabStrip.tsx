import { Eye, GitCompareArrows, Pin, X } from "lucide-react";

import {
  type AgentArtifactFollowRecord,
  type AgentArtifactTarget,
  useAgentArtifactFollowStore,
} from "../app/agentArtifactFollowStore";
import type { AcpChatPage } from "../domain/acpProjection";
import type { AgentArtifactOwner } from "../domain/agentArtifact";
import type { WorkbenchTab } from "../domain/workbench";
import { FileKindIcon } from "../features/files/FileKindIcon";
import { artifactChipStatsForTab } from "./artifactChipStats";
import { artifactTabParts } from "./agentArtifactTabPresentation";

type AgentArtifactTabStripProps = {
  artifacts: readonly WorkbenchTab[];
  selectedTarget: AgentArtifactTarget | null;
  focusedOwner: AgentArtifactOwner | null;
  focusedPage: AcpChatPage | null;
  previewSelected: boolean;
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
  record,
  availableTargets,
  activateTab,
  closeTab,
}: AgentArtifactTabStripProps) {
  return (
    <>
      <nav className="tab-strip" aria-label="Agent artifact tabs">
        <div className="tab-strip__scroll">
          {artifacts.map((tab) => {
            const selected =
              selectedTarget?.kind === "tab" && tab.id === selectedTarget.tabId;
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
                  data-resource-root-id={tab.resource?.rootId}
                  data-resource-relative-path={tab.resource?.relativePath}
                  title={fullPath}
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
                onClick={() =>
                  useAgentArtifactFollowStore
                    .getState()
                    .pin(focusedOwner, { kind: "preview" })
                }
              >
                <Eye size={15} strokeWidth={1.8} aria-hidden="true" />
                <span>Preview</span>
              </button>
            </div>
          ) : null}
        </div>
      </nav>
      {focusedOwner && record && availableTargets.length > 1 ? (
        <button
          className="agent-artifact-pane__follow"
          type="button"
          aria-label="Follow latest resource"
          aria-pressed={record.mode === "follow"}
          title={
            record.mode === "follow"
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
    </>
  );
}
