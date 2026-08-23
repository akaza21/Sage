import { useCallback, useEffect, useRef, useState } from "react";

import { useWorkbenchStore } from "../../app/store";
import {
  discoverPreviewTargets,
  getPreviewHealth,
  startPreviewTarget,
  stopPreviewTarget,
  type PreviewTargetRef,
} from "./previewDesktop";

export function PreviewSurface() {
  const project = useWorkbenchStore((state) => state.project);
  const projectGeneration = useWorkbenchStore(
    (state) => state.projectGeneration,
  );
  const epoch = useWorkbenchStore((state) => state.nativeTransitionEpoch);
  const transitioning = useWorkbenchStore(
    (state) => state.projectTransitioning,
  );
  const [targets, setTargets] = useState<PreviewTargetRef[]>([]);
  const [active, setActive] = useState<PreviewTargetRef | null>(null);
  const [message, setMessage] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);
  const [starting, setStarting] = useState(false);
  const activeTargetRef = useRef<PreviewTargetRef | null>(null);
  const startingTargetRef = useRef<PreviewTargetRef | null>(null);

  const discover = useCallback(async () => {
    if (!project || transitioning) return;
    setLoading(true);
    setMessage(null);
    try {
      const response = await discoverPreviewTargets({
        projectId: project.id,
        projectGeneration,
        expectedTransitionEpoch: epoch,
      });
      setTargets(response.targets);
    } catch (error) {
      setMessage(
        error instanceof Error
          ? error.message
          : "Preview discovery failed safely.",
      );
    } finally {
      setLoading(false);
    }
  }, [epoch, project, projectGeneration, transitioning]);

  useEffect(() => {
    const timer = window.setTimeout(() => void discover(), 0);
    return () => window.clearTimeout(timer);
  }, [discover]);

  useEffect(() => {
    return () => {
      const target = activeTargetRef.current ?? startingTargetRef.current;
      if (target) void stopPreviewTarget(target).catch(() => undefined);
    };
  }, []);

  return (
    <section className="placeholder-surface" aria-label="Preview">
      <div className="sidebar__heading">
        <span>Preview</span>
        <button
          type="button"
          onClick={() => void discover()}
          disabled={loading || transitioning}
        >
          Refresh
        </button>
      </div>
      {message ? <p role="alert">{message}</p> : null}
      {targets.length === 0 ? (
        <p className="sidebar__empty">No dev or start scripts were found.</p>
      ) : null}
      {targets.map((target) => (
        <div key={`${target.rootId}:${target.scriptName}`}>
          <span>{target.scriptName}</span>
          {active?.rootId === target.rootId &&
          active.scriptName === target.scriptName ? (
            <button
              type="button"
              onClick={() =>
                void stopPreviewTarget(target)
                  .then(() => {
                    activeTargetRef.current = null;
                    setActive(null);
                  })
                  .catch((error) =>
                    setMessage(
                      error instanceof Error
                        ? error.message
                        : "Preview stop failed safely.",
                    ),
                  )
              }
            >
              Stop
            </button>
          ) : (
            <button
              type="button"
              disabled={loading || starting || transitioning}
              onClick={() => {
                if (startingTargetRef.current) return;
                startingTargetRef.current = target;
                setStarting(true);
                void startPreviewTarget(target)
                  .then(async (started) => {
                    await getPreviewHealth(target);
                    activeTargetRef.current = started.ref;
                    startingTargetRef.current = null;
                    setStarting(false);
                    setActive(started.ref);
                  })
                  .catch((error) => {
                    const failedTarget = startingTargetRef.current;
                    startingTargetRef.current = null;
                    setStarting(false);
                    if (failedTarget) {
                      void stopPreviewTarget(failedTarget).catch(
                        () => undefined,
                      );
                    }
                    setMessage(
                      error instanceof Error
                        ? error.message
                        : "Preview start failed safely.",
                    );
                  });
              }}
            >
              Start
            </button>
          )}
        </div>
      ))}
    </section>
  );
}
