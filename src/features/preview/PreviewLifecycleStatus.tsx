import {
  CircleAlert,
  CircleCheck,
  CircleDot,
  CircleStop,
  LoaderCircle,
  PlugZap,
} from "lucide-react";

import type {
  PreviewFailure,
  PreviewPhase,
} from "../../domain/previewProjection";
import {
  previewFailureLabel,
  previewFailureMessage,
  previewPhaseDetail,
  previewPhaseLabel,
  previewPhaseTone,
} from "./previewPresentation";

const ICONS = {
  active: CircleCheck,
  failed: CircleAlert,
  idle: CircleDot,
  starting: LoaderCircle,
  stopped: CircleStop,
} as const;

export function PreviewLifecycleStatus({
  compact = false,
  failure,
  phase,
}: {
  compact?: boolean;
  failure: PreviewFailure | null | undefined;
  phase: PreviewPhase;
}) {
  const tone = previewPhaseTone(phase);
  const terminalFailure = phase === "healthFailed";
  const presentationTone = terminalFailure
    ? "failed"
    : failure === "conflict"
      ? "stopped"
      : tone;
  const Icon = ICONS[presentationTone];
  const failureLabel = previewFailureLabel(failure);
  const title = terminalFailure ? "Preview failed" : previewPhaseLabel(phase);
  const detail = previewFailureMessage(failure) ?? previewPhaseDetail(phase);

  return (
    <div
      className={`agent-preview-lifecycle agent-preview-lifecycle--${presentationTone}${
        compact ? " agent-preview-lifecycle--compact" : ""
      }`}
      role={presentationTone === "failed" ? "alert" : "status"}
      aria-atomic="true"
    >
      <span className="agent-preview-lifecycle__icon" aria-hidden="true">
        {phase === "waitingForOrigin" ? (
          <PlugZap size={16} strokeWidth={1.8} />
        ) : (
          <Icon size={16} strokeWidth={1.8} />
        )}
      </span>
      <span className="agent-preview-lifecycle__copy">
        <span className="agent-preview-lifecycle__line">
          <strong>{title}</strong>
          {failureLabel ? (
            <span className="agent-preview-lifecycle__reason">
              {failureLabel}
            </span>
          ) : null}
        </span>
        <span
          className={
            compact
              ? "agent-preview-lifecycle__detail sr-only"
              : "agent-preview-lifecycle__detail"
          }
        >
          {detail}
        </span>
      </span>
    </div>
  );
}
