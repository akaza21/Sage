import { useCallback, useEffect, useState } from "react";
import { useWorkbenchStore } from "../../app/store";
import type { PreviewTargetRef } from "../../domain/previewProjection";

/** Native children outlive Activity suspension; only logical ownership releases them. */
export function usePreviewTargetLifetime() {
  const projectId = useWorkbenchStore((state) => state.project?.id);
  const [target, setTarget] = useState<PreviewTargetRef | null>(null);
  const publish = useCallback((next: PreviewTargetRef | null) => {
    setTarget((current) =>
      targetKey(current) === targetKey(next) ? current : next,
    );
  }, []);
  useEffect(() => {
    if (!target || target.projectId !== projectId) return;
    return () => {
      void import("./previewDesktop")
        .then(({ stopPreviewTarget }) => stopPreviewTarget(target))
        .catch(() => undefined);
    };
  }, [projectId, target]);
  return publish;
}

function targetKey(target: PreviewTargetRef | null) {
  return target
    ? [
        target.projectId,
        target.projectGeneration,
        target.expectedTransitionEpoch,
        target.chatId,
        target.sessionId,
        target.runtimeEpoch,
        target.rootId,
        target.scriptName,
      ].join("\0")
    : "";
}
