import {
  useCallback,
  useEffect,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
} from "react";
import { useStore } from "zustand";

import { acpProjectionStoreFor } from "../../app/acpProjectionRegistry";
import { useWorkbenchStore } from "../../app/store";
import type { PreviewProjectionOwner } from "../../domain/previewProjection";
import { applyBrowseIntent } from "./harnessBrowse";
import {
  discoverPreviewTargets,
  startPreviewTarget,
  stopPreviewTarget,
  type PreviewTargetRef,
} from "./previewDesktop";
import { usePreviewProjection } from "./previewProjectionStore";
import { useHarnessBrowseOwner } from "./useHarnessBrowseSession";

function sameOwner(
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

function message(error: unknown): string {
  return error instanceof Error
    ? error.message
    : "Preview request failed safely.";
}

function failureMessage(failure: string | null | undefined): string | null {
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

export function PreviewSurface() {
  const project = useWorkbenchStore((state) => state.project);
  const nativeTransitionEpoch = useWorkbenchStore(
    (state) => state.nativeTransitionEpoch,
  );
  const transitioning = useWorkbenchStore(
    (state) => state.projectTransitioning,
  );
  const browseOwner = useHarnessBrowseOwner();
  const acpStore = acpProjectionStoreFor(browseOwner?.chatId);
  const page = useStore(acpStore, (state) => state.page);
  const scope = useStore(acpStore, (state) => state.scope);
  const owner = useMemo<PreviewProjectionOwner | null>(() => {
    const session = page?.sessions.find(
      (candidate) => candidate.id === browseOwner?.sessionId,
    );
    if (
      transitioning ||
      !project ||
      !browseOwner ||
      page?.projectId !== project.id ||
      page.chatId !== browseOwner.chatId ||
      page.currentSessionId !== browseOwner.sessionId ||
      !scope ||
      scope.projectId !== page.projectId ||
      scope.chatId !== page.chatId ||
      scope.projectGeneration !== browseOwner.projectGeneration ||
      !session?.runtimeEpoch ||
      session.runtimeEpoch !== browseOwner.runtimeEpoch ||
      !Number.isSafeInteger(nativeTransitionEpoch) ||
      nativeTransitionEpoch < 0 ||
      browseOwner.projectGeneration !== String(nativeTransitionEpoch) ||
      browseOwner.transitionEpoch !== nativeTransitionEpoch
    ) {
      return null;
    }
    return {
      ...browseOwner,
    };
  }, [browseOwner, nativeTransitionEpoch, page, project, scope, transitioning]);
  return (
    <PreviewSession
      key={owner ? JSON.stringify(Object.values(owner)) : "unavailable"}
      owner={owner}
      transitioning={transitioning}
    />
  );
}

function PreviewSession({
  owner,
  transitioning,
}: {
  owner: PreviewProjectionOwner | null;
  transitioning: boolean;
}) {
  const { projection, error: subscriptionError } = usePreviewProjection(owner);
  const browseOwner = useHarnessBrowseOwner();
  const [targets, setTargets] = useState<PreviewTargetRef[]>([]);
  const [discovered, setDiscovered] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const ownerRef = useRef(owner);
  const targetRef = useRef<PreviewTargetRef | null>(null);
  useLayoutEffect(() => {
    ownerRef.current = owner;
  }, [owner]);
  useEffect(() => {
    targetRef.current = projection?.target ?? null;
  }, [projection?.target]);

  const discover = useCallback(async () => {
    if (!owner) return;
    const captured = owner;
    setError(null);
    try {
      const response = await discoverPreviewTargets({
        projectId: captured.projectId,
        projectGeneration: captured.projectGeneration,
        expectedTransitionEpoch: captured.transitionEpoch,
        chatId: captured.chatId,
        sessionId: captured.sessionId,
        runtimeEpoch: captured.runtimeEpoch,
      });
      if (!sameOwner(captured, ownerRef.current)) return;
      setTargets(response.targets);
      setDiscovered(true);
    } catch (reason) {
      if (sameOwner(captured, ownerRef.current)) setError(message(reason));
    }
  }, [owner]);

  useEffect(() => {
    queueMicrotask(() => void discover());
  }, [discover]);

  useEffect(() => {
    if (
      !owner ||
      !projection ||
      projection.phase !== "active" ||
      !projection.origin
    )
      return;
    if (sameOwner(owner, browseOwner)) {
      applyBrowseIntent({ owner, url: projection.origin });
    }
  }, [browseOwner, owner, projection]);

  useEffect(() => {
    return () => {
      const target = targetRef.current;
      if (target) void stopPreviewTarget(target).catch(() => undefined);
    };
  }, []);

  const activeTarget = projection?.target ?? null;
  const busy = Boolean(
    projection &&
    [
      "discovering",
      "starting",
      "waitingForOrigin",
      "healthChecking",
      "stopping",
    ].includes(projection.phase),
  );

  return (
    <section className="placeholder-surface" aria-label="Preview">
      <div className="sidebar__heading">
        <span>Preview</span>
        <button
          type="button"
          onClick={() => void discover()}
          disabled={!owner || busy || transitioning}
        >
          Refresh
        </button>
      </div>
      {error || subscriptionError || failureMessage(projection?.failure) ? (
        <p role="alert">
          {error ?? subscriptionError ?? failureMessage(projection?.failure)}
        </p>
      ) : null}
      {discovered &&
      targets.length === 0 &&
      projection?.phase !== "discovering" ? (
        <p className="sidebar__empty">No dev or start scripts were found.</p>
      ) : null}
      {targets.map((target) => {
        const selected =
          activeTarget?.rootId === target.rootId &&
          activeTarget.scriptName === target.scriptName;
        return (
          <div key={`${target.rootId}:${target.scriptName}`}>
            <span>{target.scriptName}</span>
            {selected && projection?.phase === "active" ? (
              <button
                type="button"
                onClick={() => {
                  const captured = owner;
                  if (!captured) return;
                  void stopPreviewTarget(target).catch((reason) => {
                    if (sameOwner(captured, ownerRef.current))
                      setError(message(reason));
                  });
                }}
              >
                Stop
              </button>
            ) : (
              <button
                type="button"
                disabled={!owner || busy || transitioning}
                onClick={() => {
                  const captured = owner;
                  if (!captured) return;
                  setError(null);
                  void startPreviewTarget(target).catch((reason) => {
                    if (sameOwner(captured, ownerRef.current))
                      setError(message(reason));
                  });
                }}
              >
                Start
              </button>
            )}
          </div>
        );
      })}
    </section>
  );
}
