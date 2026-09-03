import { useCallback, useEffect, useRef, useState } from "react";

import { useWorkbenchStore } from "../../app/store";
import { applyBrowseIntent, harnessBrowseOwner } from "./harnessBrowse";
import {
  sameAgentArtifactOwner,
  type AgentArtifactOwner,
} from "../../domain/agentArtifact";
import { agentArtifactOwnerIsCurrent } from "../../app/openAgentArtifact";
import { useHarnessBrowseOwner } from "./useHarnessBrowseSession";
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
  const browseOwner = useHarnessBrowseOwner();

  return (
    <PreviewSession
      key={`${project?.id ?? "no-project"}:${projectGeneration}:${epoch}`}
      project={project}
      projectGeneration={projectGeneration}
      epoch={epoch}
      transitioning={transitioning}
      browseOwner={browseOwner}
    />
  );
}

type PreviewSessionProps = {
  project: { id: string } | null;
  projectGeneration: number;
  epoch: number;
  transitioning: boolean;
  browseOwner: AgentArtifactOwner | null;
};

function matchesPreviewTarget(
  left: PreviewTargetRef | null,
  right: PreviewTargetRef,
): boolean {
  return (
    left?.projectId === right.projectId &&
    left.projectGeneration === right.projectGeneration &&
    left.expectedTransitionEpoch === right.expectedTransitionEpoch &&
    left.rootId === right.rootId &&
    left.scriptName === right.scriptName
  );
}

function matchesPreviewSession(
  target: PreviewTargetRef,
  project: { id: string } | null,
  projectGeneration: number,
  epoch: number,
): boolean {
  return (
    project !== null &&
    target.projectId === project.id &&
    target.projectGeneration === projectGeneration &&
    target.expectedTransitionEpoch === epoch
  );
}

function healthyOriginForTarget(
  response: Awaited<ReturnType<typeof getPreviewHealth>>,
  target: PreviewTargetRef,
): string | null {
  return response.healthy &&
    typeof response.origin === "string" &&
    matchesPreviewTarget(response.ref, target)
    ? response.origin
    : null;
}

function publishPreviewForOwner(
  owner: AgentArtifactOwner | null,
  projectGeneration: number,
  origin: string,
): void {
  if (
    owner &&
    sameAgentArtifactOwner(owner, harnessBrowseOwner()) &&
    agentArtifactOwnerIsCurrent(owner, projectGeneration)
  ) {
    applyBrowseIntent({ owner, url: origin });
  }
}

function PreviewSession({
  project,
  projectGeneration,
  epoch,
  transitioning,
  browseOwner,
}: PreviewSessionProps) {
  const [targets, setTargets] = useState<PreviewTargetRef[]>([]);
  const [active, setActive] = useState<PreviewTargetRef | null>(null);
  const [message, setMessage] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);
  const [starting, setStarting] = useState(false);
  const activeTargetRef = useRef<PreviewTargetRef | null>(null);
  const startingTargetRef = useRef<PreviewTargetRef | null>(null);
  const mountedRef = useRef(false);

  const discover = useCallback(async () => {
    if (!project || transitioning) return;
    const discoveryOwner = harnessBrowseOwner();
    setLoading(true);
    setMessage(null);
    try {
      const response = await discoverPreviewTargets({
        projectId: project.id,
        projectGeneration,
        expectedTransitionEpoch: epoch,
      });
      if (!mountedRef.current) return;
      if (
        !response.targets.every((target) =>
          matchesPreviewSession(target, project, projectGeneration, epoch),
        )
      ) {
        throw new Error("Preview discovery returned a stale target.");
      }
      setTargets(response.targets);
      let recovered: { ref: PreviewTargetRef; origin: string } | null = null;
      for (const target of response.targets) {
        let health: Awaited<ReturnType<typeof getPreviewHealth>>;
        try {
          health = await getPreviewHealth(target);
        } catch {
          continue;
        }
        if (!mountedRef.current) return;
        const origin = healthyOriginForTarget(health, target);
        if (origin === null) continue;
        recovered = { ref: health.ref, origin };
        break;
      }
      if (!mountedRef.current) return;
      activeTargetRef.current = recovered?.ref ?? null;
      setActive(recovered?.ref ?? null);
      if (recovered) {
        publishPreviewForOwner(
          discoveryOwner,
          projectGeneration,
          recovered.origin,
        );
      }
    } catch (error) {
      if (!mountedRef.current) return;
      setMessage(
        error instanceof Error
          ? error.message
          : "Preview discovery failed safely.",
      );
    } finally {
      if (mountedRef.current) setLoading(false);
    }
  }, [epoch, project, projectGeneration, transitioning]);

  useEffect(() => {
    const timer = window.setTimeout(() => void discover(), 0);
    return () => window.clearTimeout(timer);
  }, [discover]);

  useEffect(() => {
    mountedRef.current = true;
    return () => {
      mountedRef.current = false;
      const target = activeTargetRef.current ?? startingTargetRef.current;
      activeTargetRef.current = null;
      startingTargetRef.current = null;
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
          {matchesPreviewTarget(active, target) ? (
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
                let startedTarget: PreviewTargetRef | null = null;
                void startPreviewTarget(target)
                  .then(async (started) => {
                    startedTarget = started.ref;
                    if (!mountedRef.current) return;
                    if (!matchesPreviewTarget(started.ref, target)) {
                      throw new Error("Preview start returned a stale target.");
                    }
                    const health = await getPreviewHealth(target);
                    if (!mountedRef.current) return;
                    const healthyOrigin = healthyOriginForTarget(
                      health,
                      target,
                    );
                    if (healthyOrigin === null) {
                      throw new Error("Preview did not become healthy.");
                    }
                    activeTargetRef.current = started.ref;
                    startingTargetRef.current = null;
                    setStarting(false);
                    setActive(started.ref);
                    publishPreviewForOwner(
                      browseOwner,
                      projectGeneration,
                      healthyOrigin,
                    );
                  })
                  .catch((error) => {
                    if (!mountedRef.current) return;
                    startingTargetRef.current = null;
                    setStarting(false);
                    if (startedTarget) {
                      void stopPreviewTarget(startedTarget).catch(
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
