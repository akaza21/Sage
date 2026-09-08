import {
  useCallback,
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
} from "react";
import { createPortal } from "react-dom";

import type { PreviewProjectionOwner } from "../../domain/previewProjection";
import { LazyModuleBoundary } from "../../ui/LazyModuleBoundary";
import { applyBrowseIntent } from "./harnessBrowse";
import type { HarnessBrowseOwner } from "./harnessBrowse";
import {
  previewOrigin,
  previewOwnerKey,
  samePreviewOwner,
} from "./previewPresentation";
import {
  discoverPreviewTargets,
  getPreviewHealth,
  previewDesktopAvailable,
  startPreviewTarget,
  stopPreviewTarget,
  type PreviewTargetRef,
} from "./previewDesktop";
import { usePreviewProjection } from "./previewProjectionStore";

const loadAgentPreviewPresentation = () =>
  import("./AgentPreviewPresentation").then(({ AgentPreviewPresentation }) => ({
    default: AgentPreviewPresentation,
  }));

/**
 * A lightweight exact-owner controller. It intentionally stays mounted while
 * the Agent tab is hidden so visibility changes do not rediscover or stop the
 * native child; only the browser canvas below is slept.
 */
export function AgentPreviewSurface({
  owner,
  browseOwner,
  visible,
  container,
}: {
  owner: PreviewProjectionOwner | null;
  browseOwner: HarnessBrowseOwner | null;
  visible: boolean;
  container: HTMLElement | null;
}) {
  const {
    error: subscriptionError,
    listenerReady,
    projection,
  } = usePreviewProjection(owner);
  const desktopAvailable = previewDesktopAvailable();
  const [targets, setTargets] = useState<PreviewTargetRef[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [lastAdmittedNativeOrigin, setLastAdmittedNativeOrigin] = useState<
    string | null
  >(null);
  const ownerRef = useRef(owner);
  const targetRef = useRef<PreviewTargetRef | null>(null);
  const discoveredFor = useRef<string | null>(null);
  const wasVisible = useRef(false);
  const healthInFlight = useRef<string | null>(null);
  const key = previewOwnerKey(owner);

  useLayoutEffect(() => {
    ownerRef.current = owner;
  }, [owner]);
  useEffect(() => {
    targetRef.current = projection?.target ?? null;
  }, [projection?.target]);
  useEffect(() => {
    if (
      !owner ||
      !visible ||
      !desktopAvailable ||
      !listenerReady ||
      discoveredFor.current === key
    )
      return;
    discoveredFor.current = key;
    const captured = owner;
    void discoverPreviewTargets({
      projectId: captured.projectId,
      projectGeneration: captured.projectGeneration,
      expectedTransitionEpoch: captured.transitionEpoch,
      chatId: captured.chatId,
      sessionId: captured.sessionId,
      runtimeEpoch: captured.runtimeEpoch,
    })
      .then((response) => {
        if (samePreviewOwner(captured, ownerRef.current))
          setTargets(response.targets);
      })
      .catch((reason: unknown) => {
        if (samePreviewOwner(captured, ownerRef.current))
          setError(
            reason instanceof Error
              ? reason.message
              : "Preview request failed safely.",
          );
      });
  }, [desktopAvailable, key, listenerReady, owner, visible]);

  useEffect(() => {
    if (
      !owner ||
      !projection ||
      projection.phase !== "active" ||
      !projection.origin
    )
      return;
    const origin = previewOrigin(projection.origin);
    if (!origin || lastAdmittedNativeOrigin === origin) return;
    // A health revalidation can emit active again. Do not let that revision
    // replace a manually opened page at a different origin once this native
    // origin has already been admitted by the controller.
    const admitted = applyBrowseIntent({ owner, url: projection.origin });
    if (admitted) queueMicrotask(() => setLastAdmittedNativeOrigin(origin));
  }, [lastAdmittedNativeOrigin, owner, projection]);

  // This is deliberately one coalesced check on an actual hidden-to-visible
  // resume of a known active target. It is not keyed to revisions: the health
  // event sequence itself may revise active, and must never create a loop.
  useEffect(() => {
    if (!visible) {
      wasVisible.current = false;
      return;
    }
    const resumed = !wasVisible.current;
    wasVisible.current = true;
    if (
      !desktopAvailable ||
      !resumed ||
      projection?.phase !== "active" ||
      !projection.target
    )
      return;
    const healthKey = `${key}:${projection.target.rootId}:${projection.target.scriptName}`;
    if (healthInFlight.current) return;
    healthInFlight.current = healthKey;
    void getPreviewHealth(projection.target)
      .catch(() => undefined)
      .finally(() => {
        if (healthInFlight.current === healthKey) healthInFlight.current = null;
      });
  }, [desktopAvailable, key, projection?.phase, projection?.target, visible]);

  useEffect(() => {
    return () => {
      const target = targetRef.current;
      if (target) void stopPreviewTarget(target).catch(() => undefined);
    };
  }, []);

  const onStart = useCallback(
    (target: PreviewTargetRef) => {
      if (!owner) return;
      void startPreviewTarget(target).catch(
        (reason: unknown) =>
          samePreviewOwner(owner, ownerRef.current) &&
          setError(
            reason instanceof Error
              ? reason.message
              : "Preview request failed safely.",
          ),
      );
    },
    [owner],
  );
  const onStop = useCallback((target: PreviewTargetRef) => {
    void stopPreviewTarget(target).catch(() => undefined);
  }, []);

  if (!visible || !container) return null;

  return createPortal(
    <LazyModuleBoundary
      label="Preview"
      load={loadAgentPreviewPresentation}
      render={(AgentPreviewPresentation) => (
        <AgentPreviewPresentation
          owner={owner}
          browseOwner={browseOwner}
          desktopAvailable={desktopAvailable}
          listenerReady={listenerReady}
          projection={projection}
          targets={targets}
          error={error ?? (desktopAvailable ? subscriptionError : null)}
          lastAdmittedNativeOrigin={lastAdmittedNativeOrigin}
          onStart={onStart}
          onStop={onStop}
        />
      )}
    />,
    container,
  );
}
