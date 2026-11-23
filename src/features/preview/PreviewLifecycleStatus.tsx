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
  const failed = Boolean(failure) || phase === "healthFailed";
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
        identity={`preview:${scope}:${failed ? (failure ?? phase) : "lifecycle"}`}
        severity={failed ? "warning" : "info"}
        message={message}
        // Only a failed health check interrupts, as before; an exited or
        // conflicting Preview is a standing state that replays on remount.
        announce={phase === "healthFailed"}
        progress={!failed}
        action={
          failed && onRestart && (!failure || RESTARTABLE.has(failure))
            ? { label: "Start again", onClick: onRestart }
            : undefined
        }
      />
    </div>
  );
}
