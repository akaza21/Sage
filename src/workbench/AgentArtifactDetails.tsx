import { Check, Copy, ChevronLeft, ChevronRight } from "../ui/leanIcons";
import { useEffect, useState, type Ref } from "react";
import type { artifactChipStatsForTab } from "./artifactChipStats";

export function AgentArtifactDetails({
  fileDetailsRef,
  admitted,
  path,
  position,
  total,
  onStep,
  stats,
}: {
  fileDetailsRef?: Ref<HTMLDivElement>;
  admitted: boolean;
  path: string;
  position: number;
  total: number;
  onStep: (direction: -1 | 1) => void;
  stats: ReturnType<typeof artifactChipStatsForTab>;
}) {
  const [copyState, setCopyState] = useState<"idle" | "copied" | "error">(
    "idle",
  );
  const [feedbackRevision, setFeedbackRevision] = useState(0);
  useEffect(() => {
    if (copyState === "idle") return;
    const timer = setTimeout(() => setCopyState("idle"), COPY_FEEDBACK_MS);
    return () => clearTimeout(timer);
  }, [copyState, feedbackRevision]);
  async function copyPath() {
    try {
      await navigator.clipboard.writeText(path);
      setCopyState("copied");
    } catch {
      setCopyState("error");
    }
    setFeedbackRevision((revision) => revision + 1);
  }
  const canStep = position >= 0 && total > 1;
  return (
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
        title={copyState === "error" ? "Could not copy path" : "Copy path"}
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
  );
}
const COPY_FEEDBACK_MS = 2_000;
