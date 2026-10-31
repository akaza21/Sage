import {
  ChevronLeft,
  Copy,
  SquareArrowOutUpRight,
  Check,
  ChevronDown,
  ChevronRight,
} from "../ui/leanIcons";

import {
  lazy,
  Suspense,
  useEffect,
  useId,
  useRef,
  useState,
  type Ref,
} from "react";

import type { AcpChatPage } from "../domain/acpProjection";
import type { WorkbenchTab } from "../domain/workbench";
import { FilePathChips } from "../features/files/FilePathChips";
import {
  useAgentDiffSourceStore,
  openAgentSnapshotSource,
} from "../app/agentDiffSource";
import { openAgentArtifact } from "../app/openAgentArtifact";
import { useWorkbenchStore } from "../app/store";
import { artifactChipStatsForTab } from "./artifactChipStats";
import { artifactTabParts } from "./agentArtifactTabPresentation";

const AgentArtifactOpenMenu = lazy(() =>
  import("./AgentArtifactOpenMenu").then((module) => ({
    default: module.AgentArtifactOpenMenu,
  })),
);
const COPY_FEEDBACK_MS = 2_000;

export function AgentArtifactHeader({
  tab,
  admitted,
  page,
  position,
  total,
  onStep,
  onOpenInEditor,
  fileActionsRef,
  fileDetailsRef,
}: {
  tab: WorkbenchTab;
  admitted: boolean;
  page: AcpChatPage | null;
  position: number;
  total: number;
  onStep: (direction: -1 | 1) => void;
  onOpenInEditor: () => void;
  fileActionsRef?: Ref<HTMLDivElement>;
  fileDetailsRef?: Ref<HTMLDivElement>;
}) {
  const [copyState, setCopyState] = useState<"idle" | "copied" | "error">(
    "idle",
  );
  const [copyFeedbackRevision, setCopyFeedbackRevision] = useState(0);
  const [openChoices, setOpenChoices] = useState(false);
  const [error, setError] = useState("");
  const savedSource = useAgentDiffSourceStore((state) =>
    state.sourceReturns.find(
      (entry) =>
        entry.tabId === tab.id &&
        entry.owner.projectId === page?.projectId &&
        entry.owner.chatId === page.chatId &&
        entry.owner.sessionId === page.currentSessionId,
    ),
  );
  const generation = useWorkbenchStore((state) => state.projectGeneration);
  const openChoicesId = useId();
  const openChoicesTrigger = useRef<HTMLButtonElement>(null);
  useEffect(() => {
    if (copyState === "idle") return;
    const timeout = window.setTimeout(
      () => setCopyState("idle"),
      COPY_FEEDBACK_MS,
    );
    return () => window.clearTimeout(timeout);
  }, [copyState, copyFeedbackRevision]);
  const [path] = artifactTabParts(tab);
  const stats = artifactChipStatsForTab(tab, page);
  const canStep = position >= 0 && total > 1;
  async function navigateSaved(source: boolean) {
    setError("");
    try {
      const ref = source ? tab.diffRef : savedSource?.ref;
      const opened =
        ref &&
        (source
          ? await openAgentSnapshotSource(ref)
          : savedSource?.generation === generation &&
            (await openAgentArtifact({
              owner: ref,
              resource: ref,
              target: "diff",
              diffRef: ref,
            })));
      if (!opened)
        setError("This saved change is no longer available in this Chat.");
    } catch {
      setError("Could not open this resource. Try again.");
    }
  }

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
      <div className="agent-artifact-header__title-row">
        <div className="agent-artifact-header__identity" title={path}>
          {savedSource ? (
            <span className="agent-artifact-header__live">Live</span>
          ) : null}
          <FilePathChips path={path} />
        </div>
        {tab.diffRef ? (
          <button
            className="button button--compact"
            type="button"
            onClick={() => void navigateSaved(true)}
          >
            View live source
          </button>
        ) : null}
        {savedSource ? (
          <button
            className="button button--compact"
            type="button"
            onClick={() => void navigateSaved(false)}
          >
            Changes
          </button>
        ) : null}
        {tab.kind === "file" ? (
          <div
            ref={fileActionsRef}
            className="agent-artifact-header__file-actions"
          />
        ) : null}
        <details className="agent-artifact-header__details">
          <summary
            aria-label="Resource details"
            title="View only · Resource details"
          >
            View only
          </summary>
          <div className="agent-artifact-header__detail-body">
            <div ref={fileDetailsRef} />
            {admitted ? <span>Opened by Sage</span> : null}
            <code>{path}</code>
            <div className="agent-artifact-header__stepper">
              <button
                type="button"
                aria-label="Previous artifact"
                title="Previous artifact"
                disabled={!canStep || position === 0}
                onClick={() => onStep(-1)}
              >
                <ChevronLeft size={16} aria-hidden="true" />
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
                <ChevronRight size={16} aria-hidden="true" />
              </button>
            </div>
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
              title={
                copyState === "error" ? "Could not copy path" : "Copy path"
              }
              onClick={() => void copyPath()}
            >
              {copyState === "copied" ? (
                <Check size={16} aria-hidden="true" />
              ) : (
                <Copy size={16} aria-hidden="true" />
              )}
              <span>{copyState === "copied" ? "Copied" : "Copy path"}</span>
            </button>
          </div>
        </details>
        {tab.kind === "file" ? (
          <>
            <button
              ref={openChoicesTrigger}
              className="button button--compact agent-artifact-header__open"
              type="button"
              aria-label="Open choices"
              aria-controls={openChoicesId}
              aria-expanded={openChoices}
              aria-haspopup="menu"
              onClick={() => setOpenChoices((current) => !current)}
            >
              Open <ChevronDown size={12} aria-hidden="true" />
            </button>
            {openChoices ? (
              <Suspense fallback={null}>
                <AgentArtifactOpenMenu
                  id={openChoicesId}
                  trigger={openChoicesTrigger}
                  dismiss={() => setOpenChoices(false)}
                  onOpenInEditor={onOpenInEditor}
                  copyPath={copyPath}
                />
              </Suspense>
            ) : null}
          </>
        ) : (
          <button
            className="button button--compact agent-artifact-header__editor"
            type="button"
            onClick={onOpenInEditor}
          >
            <SquareArrowOutUpRight size={16} aria-hidden="true" />
            <span className="agent-artifact-header__editor-label">
              Open in Editor
            </span>
          </button>
        )}
      </div>
      {error ? <p role="alert">{error}</p> : null}
    </header>
  );
}
