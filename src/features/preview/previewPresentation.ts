import type {
  PreviewFailure,
  PreviewPhase,
} from "../../domain/previewProjection";

export type PreviewPhaseTone =
  "active" | "failed" | "idle" | "starting" | "stopped";

const FAILURE_PRESENTATION = {
  scriptMissing: {
    label: "Missing script",
    detail: "No runnable Preview script was found.",
  },
  originNotLoopback: {
    label: "Unsafe origin",
    detail: "Preview did not provide a safe loopback address.",
  },
  originUnsupported: {
    label: "Unsupported origin",
    detail: "Preview supports http:// loopback origins only.",
  },
  startupTimeout: {
    label: "Startup timeout",
    detail: "Preview did not start before its deadline.",
  },
  healthTimeout: {
    label: "Health timeout",
    detail: "Preview did not become healthy before its deadline.",
  },
  exitedEarly: {
    label: "Early exit",
    detail: "Preview exited before it became ready.",
  },
  conflict: {
    label: "Ownership conflict",
    detail: "Another Preview operation is already active.",
  },
  internal: {
    label: "Internal error",
    detail: "Preview failed safely.",
  },
} as const;

const PHASE_PRESENTATION: Record<
  PreviewPhase,
  { detail: string; label: string; tone: PreviewPhaseTone }
> = {
  active: {
    detail: "The loopback Preview server is ready.",
    label: "Preview active",
    tone: "active",
  },
  discovering: {
    detail: "Finding runnable Preview targets for this Chat.",
    label: "Discovering targets",
    tone: "starting",
  },
  exited: {
    detail: "Preview exited before it became ready.",
    label: "Preview exited",
    tone: "stopped",
  },
  healthChecking: {
    detail: "The loopback server responded. Confirming it is ready.",
    label: "Checking health",
    tone: "starting",
  },
  healthFailed: {
    detail: "Preview failed safely.",
    label: "Preview failed",
    tone: "failed",
  },
  idle: {
    detail: "Start a discovered Preview target to open its loopback server.",
    label: "Preview idle",
    tone: "idle",
  },
  starting: {
    detail: "Launching the selected Preview process.",
    label: "Starting process",
    tone: "starting",
  },
  stopped: {
    detail: "Preview has stopped. Start it again when you are ready.",
    label: "Preview stopped",
    tone: "stopped",
  },
  stopping: {
    detail: "Stopping the native Preview process.",
    label: "Stopping process",
    tone: "starting",
  },
  waitingForOrigin: {
    detail: "The process started. Waiting for its safe loopback address.",
    label: "Waiting for origin",
    tone: "starting",
  },
};

export function previewFailureLabel(
  failure: PreviewFailure | null | undefined,
): string | null {
  return failure ? FAILURE_PRESENTATION[failure].label : null;
}

export function previewFailureMessage(
  failure: PreviewFailure | null | undefined,
): string | null {
  return failure ? FAILURE_PRESENTATION[failure].detail : null;
}

export function previewPhaseDetail(phase: PreviewPhase): string {
  return PHASE_PRESENTATION[phase].detail;
}

export function previewPhaseLabel(phase: PreviewPhase): string {
  return PHASE_PRESENTATION[phase].label;
}

export function previewPhaseTone(phase: PreviewPhase): PreviewPhaseTone {
  return PHASE_PRESENTATION[phase].tone;
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
