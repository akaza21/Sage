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
import { previewFailureMessage, samePreviewOwner } from "./previewPresentation";
import { usePreviewProjection } from "./previewProjectionStore";
import { useHarnessBrowseOwner } from "./useHarnessBrowseSession";

function message(error: unknown): string {
  return error instanceof Error
    ? error.message
    : "Preview request failed safely.";
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
  const {
    projection,
    error: subscriptionError,
    listenerReady,
  } = usePreviewProjection(owner);
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
      if (!samePreviewOwner(captured, ownerRef.current)) return;
      setTargets(response.targets);
      setDiscovered(true);
    } catch (reason) {
      if (samePreviewOwner(captured, ownerRef.current))
        setError(message(reason));
    }
  }, [owner]);

  useEffect(() => {
    if (!listenerReady) return;
    queueMicrotask(() => void discover());
  }, [discover, listenerReady]);

  useEffect(() => {
    if (
      !owner ||
      !projection ||
      projection.phase !== "active" ||
      !projection.origin
    )
      return;
    if (samePreviewOwner(owner, browseOwner)) {
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
          disabled={!owner || !listenerReady || busy || transitioning}
        >
          Refresh
        </button>
      </div>
      {error ||
      subscriptionError ||
      previewFailureMessage(projection?.failure) ? (
        <p role="alert">
          {error ??
            subscriptionError ??
            previewFailureMessage(projection?.failure)}
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
                    if (samePreviewOwner(captured, ownerRef.current))
                      setError(message(reason));
                  });
                }}
              >
                Stop
              </button>
            ) : (
              <button
                type="button"
                disabled={!owner || !listenerReady || busy || transitioning}
                onClick={() => {
                  const captured = owner;
                  if (!captured) return;
                  setError(null);
                  void startPreviewTarget(target).catch((reason) => {
                    if (samePreviewOwner(captured, ownerRef.current))
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
