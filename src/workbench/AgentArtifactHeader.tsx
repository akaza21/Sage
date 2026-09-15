import {
  Check,
  ChevronLeft,
  ChevronRight,
  Copy,
  FileDiff,
  SquareArrowOutUpRight,
} from "lucide-react";
import { useEffect, useState } from "react";

import type { AcpChatPage } from "../domain/acpProjection";
import type { WorkbenchTab } from "../domain/workbench";
import { FileKindIcon } from "../features/files/FileKindIcon";
import { artifactChipStatsForTab } from "./artifactChipStats";
import { artifactTabParts } from "./agentArtifactTabPresentation";

export function AgentArtifactHeader({
  tab,
  admitted,
  page,
  position,
  total,
  onStep,
  onOpenInEditor,
}: {
  tab: WorkbenchTab;
  admitted: boolean;
  page: AcpChatPage | null;
  position: number;
  total: number;
  onStep: (direction: -1 | 1) => void;
  onOpenInEditor: () => void;
}) {
  const [copyState, setCopyState] = useState<"idle" | "copied" | "error">(
    "idle",
  );
  const [copyFeedbackRevision, setCopyFeedbackRevision] = useState(0);
  useEffect(() => {
    if (copyState === "idle") return;
    const timeout = window.setTimeout(() => setCopyState("idle"), 2_000);
    return () => window.clearTimeout(timeout);
  }, [copyState, copyFeedbackRevision]);
  const [path, basename] = artifactTabParts(tab);
  const stats = artifactChipStatsForTab(tab, page);
  const canStep = position >= 0 && total > 1;
  const kindLabel = tab.kind === "diff" ? "Diff" : "File";

  async function copyPath() {
    try {
      await navigator.clipboard.writeText(path);
      setCopyState("copied");
      setCopyFeedbackRevision((revision) => revision + 1);
    } catch {
      setCopyState("error");
      setCopyFeedbackRevision((revision) => revision + 1);
    }
  }

  return (
    <header className="agent-artifact-header">
      {admitted ? (
        <span className="agent-artifact-header__provenance">
          Opened by Sage
        </span>
      ) : null}
      <div className="agent-artifact-header__title-row">
        <div className="agent-artifact-header__identity">
          {tab.kind === "file" ? (
            <FileKindIcon path={path} size={16} />
          ) : (
            <FileDiff size={16} strokeWidth={1.8} aria-hidden="true" />
          )}
          <span className="agent-artifact-header__kind">{kindLabel}</span>
          <strong title={path}>{basename}</strong>
        </div>
        <span className="agent-artifact-header__view-only">VIEW ONLY</span>
        <div className="agent-artifact-header__stepper">
          <button
            type="button"
            aria-label="Previous artifact"
            title="Previous artifact"
            disabled={!canStep || position === 0}
            onClick={() => onStep(-1)}
          >
            <ChevronLeft size={15} aria-hidden="true" />
          </button>
          <span aria-live="polite" aria-atomic="true">
            {position >= 0
              ? `${position + 1} of ${total}`
              : "No artifact selected"}
          </span>
          <button
            type="button"
            aria-label="Next artifact"
            title="Next artifact"
            disabled={!canStep || position === total - 1}
            onClick={() => onStep(1)}
          >
            <ChevronRight size={15} aria-hidden="true" />
          </button>
        </div>
        <button
          className="button button--compact agent-artifact-header__editor"
          type="button"
          onClick={onOpenInEditor}
        >
          <SquareArrowOutUpRight size={15} aria-hidden="true" />
          <span className="agent-artifact-header__editor-label">
            Open in Editor
          </span>
        </button>
      </div>
      <div className="agent-artifact-header__path-row">
        <code title={path}>{path}</code>
        {stats?.additionsLabel ? (
          <span className="agent-artifact-header__additions">
            {stats.additionsLabel}
          </span>
        ) : null}
        {stats?.deletionsLabel ? (
          <span className="agent-artifact-header__deletions">
            {stats.deletionsLabel}
          </span>
        ) : null}
        <button
          className="agent-artifact-header__copy"
          type="button"
          aria-label={
            copyState === "copied"
              ? "Path copied"
              : copyState === "error"
                ? "Could not copy path"
                : "Copy path"
          }
          title={copyState === "error" ? "Could not copy path" : "Copy path"}
          onClick={() => void copyPath()}
        >
          {copyState === "copied" ? (
            <Check size={15} aria-hidden="true" />
          ) : (
            <Copy size={15} aria-hidden="true" />
          )}
          <span>{copyState === "copied" ? "Copied" : "Copy path"}</span>
        </button>
      </div>
    </header>
  );
}
