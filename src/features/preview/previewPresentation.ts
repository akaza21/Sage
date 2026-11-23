import type {
  PreviewFailure,
  PreviewPhase,
} from "../../domain/previewProjection";

const FAILURE_DETAIL: Record<PreviewFailure, string> = {
  scriptMissing: "No runnable Preview script was found.",
  originNotLoopback: "Preview did not provide a safe loopback address.",
  originUnsupported: "Preview supports http:// loopback origins only.",
  startupTimeout: "Preview did not start before its deadline.",
  healthTimeout: "Preview did not become healthy before its deadline.",
  exitedEarly: "Preview exited before it became ready.",
  conflict: "Another Preview operation is already active.",
  internal: "Preview failed safely.",
};

const PHASE_PRESENTATION: Record<
  PreviewPhase,
  { detail: string; label: string }
> = {
  active: {
    detail: "The loopback Preview server is ready.",
    label: "Preview active",
  },
  discovering: {
    detail: "Finding runnable preview targets for this chat.",
    label: "Discovering targets",
  },
  exited: {
    detail: "Preview exited before it became ready.",
    label: "Preview exited",
  },
  healthChecking: {
    detail: "The loopback server responded. Confirming it is ready.",
    label: "Checking health",
  },
  healthFailed: {
    detail: "Preview failed safely.",
    label: "Preview failed",
  },
  idle: {
    detail: "Start a discovered Preview target to open its loopback server.",
    label: "Preview idle",
  },
  starting: {
    detail: "Launching the selected Preview process.",
    label: "Starting process",
  },
  stopped: {
    detail: "Preview has stopped. Start it again when you are ready.",
    label: "Preview stopped",
  },
  stopping: {
    detail: "Stopping the native Preview process.",
    label: "Stopping process",
  },
  waitingForOrigin: {
    detail: "The process started. Waiting for its safe loopback address.",
    label: "Waiting for origin",
  },
};

export function previewFailureMessage(
  failure: PreviewFailure | null | undefined,
): string | null {
  return failure ? FAILURE_DETAIL[failure] : null;
}

export function previewPhaseDetail(phase: PreviewPhase): string {
  return PHASE_PRESENTATION[phase].detail;
}

export function previewPhaseLabel(phase: PreviewPhase): string {
  return PHASE_PRESENTATION[phase].label;
}

export function previewOrigin(value: string): string | null {
  try {
    return new URL(value).origin;
  } catch {
    return null;
  }
}

export {
  samePreviewOwner,
  previewOwnerKey,
} from "../../domain/previewProjection";
