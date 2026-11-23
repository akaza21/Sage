import type {
  PreviewFailure,
  PreviewPhase,
} from "../../domain/previewProjection";
import { Notice } from "../../ui/Notice";
import {
  previewFailureMessage,
  previewPhaseDetail,
  previewPhaseLabel,
} from "./previewPresentation";

/** Failures a fresh start can clear; the rest need a project or Sage change. */
const RESTARTABLE = new Set<PreviewFailure>([
  "startupTimeout",
  "healthTimeout",
  "exitedEarly",
  "internal",
]);

/** One Preview lifecycle notice: a failure warns once and offers one restart. */
export function PreviewLifecycleStatus({
  compact = false,
  failure,
  phase,
  scope,
  onRestart,
}: {
  compact?: boolean;
  failure: PreviewFailure | null | undefined;
  phase: PreviewPhase;
  /** The owning chat and target; a new owner is a new problem identity. */
  scope: string;
  onRestart?: () => void;
}) {
  const health = phase === "healthFailed";
  const failed = Boolean(failure) || health;
  const message =
    previewFailureMessage(failure) ??
    (compact && !failed ? previewPhaseLabel(phase) : previewPhaseDetail(phase));
  return (
    <div
      className={
        compact
          ? "agent-preview-lifecycle agent-preview-lifecycle--compact"
          : "agent-preview-lifecycle"
      }
    >
      <Notice
        // As before, only a failed health check interrupts. Every other phase,
        // exits and conflicts included, updates one polite status in place, so a
        // change is read while watching and a remount stays quiet.
        identity={`preview:${scope}${health ? ":healthFailed" : ""}`}
        severity={failed ? "warning" : "info"}
        message={message}
        announce={health}
        progress={!health}
        action={
          failed && onRestart && (!failure || RESTARTABLE.has(failure))
            ? { label: "Start again", onClick: onRestart }
            : undefined
        }
      />
    </div>
  );
}
