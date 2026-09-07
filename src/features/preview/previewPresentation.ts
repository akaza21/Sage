import type {
  PreviewPhase,
  PreviewProjectionOwner,
} from "../../domain/previewProjection";

export type PreviewNotice = {
  state: "error" | "idle" | "loading";
  title: string;
  detail: string;
};

export function samePreviewOwner(
  left: PreviewProjectionOwner | null,
  right: PreviewProjectionOwner | null,
): boolean {
  return Boolean(
    left &&
    right &&
    left.projectId === right.projectId &&
    left.projectGeneration === right.projectGeneration &&
    left.transitionEpoch === right.transitionEpoch &&
    left.chatId === right.chatId &&
    left.sessionId === right.sessionId &&
    left.runtimeEpoch === right.runtimeEpoch,
  );
}

export function previewOwnerKey(owner: PreviewProjectionOwner | null): string {
  return owner
    ? JSON.stringify([
        owner.projectId,
        owner.projectGeneration,
        owner.transitionEpoch,
        owner.chatId,
        owner.sessionId,
        owner.runtimeEpoch,
      ])
    : "";
}

export function previewFailureMessage(
  failure: string | null | undefined,
): string | null {
  if (!failure) return null;
  return (
    {
      scriptMissing: "No runnable Preview script was found.",
      originNotLoopback: "Preview did not provide a safe loopback address.",
      startupTimeout: "Preview did not start before its deadline.",
      healthTimeout: "Preview did not become healthy before its deadline.",
      exitedEarly: "Preview exited before it became ready.",
      conflict: "Another Preview operation is already active.",
      internal: "Preview failed safely.",
    }[failure] ?? "Preview failed safely."
  );
}

export function previewOrigin(value: string): string | null {
  try {
    return new URL(value).origin;
  } catch {
    return null;
  }
}

export function previewNotice(
  phase: PreviewPhase,
  failure: string | null | undefined,
): PreviewNotice {
  const failed = previewFailureMessage(failure);
  if (phase === "healthFailed") {
    return {
      state: "error",
      title: "Preview failed",
      detail: failed ?? "Preview failed safely.",
    };
  }
  if (phase === "exited") {
    return {
      state: "idle",
      title: "Preview exited",
      detail: failed ?? "Preview exited before it became ready.",
    };
  }
  if (phase === "stopped") {
    return {
      state: "idle",
      title: "Preview stopped",
      detail: "Preview has stopped. Start it again when you are ready.",
    };
  }
  if (phase === "idle") {
    return {
      state: "idle",
      title: "Preview is idle",
      detail: "Start a discovered Preview target to open its loopback server.",
    };
  }
  if (phase === "stopping") {
    return {
      state: "loading",
      title: "Preview is stopping",
      detail: "Stopping the native Preview process.",
    };
  }
  if (phase === "discovering") {
    return {
      state: "loading",
      title: "Preview is discovering targets",
      detail: "Finding runnable Preview targets for this Chat.",
    };
  }
  return {
    state: "loading",
    title: "Preview is starting",
    detail: "The native Preview lifecycle is still in progress.",
  };
}
