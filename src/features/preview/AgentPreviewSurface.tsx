import { useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import { createPortal } from "react-dom";

import type { PreviewProjectionOwner } from "../../domain/previewProjection";
import { ResourceStateNotice } from "../../ui/ResourceStateNotice";
import { HarnessBrowserSurface } from "./HarnessBrowserSurface";
import { applyBrowseIntent } from "./harnessBrowse";
import type { HarnessBrowseOwner } from "./harnessBrowse";
import {
  previewNotice,
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
import { useHarnessBrowseSession } from "./useHarnessBrowseSession";

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
  const session = useHarnessBrowseSession(browseOwner);
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
  const activeTarget = projection?.target ?? null;
  const replacesNativePresentation = Boolean(
    projection &&
    projection.phase !== "active" &&
    (!session ||
      (lastAdmittedNativeOrigin &&
        previewOrigin(session.href) === lastAdmittedNativeOrigin)),
  );
  const nativeNotice = projection
    ? previewNotice(projection.phase, projection.failure)
    : null;
  const genericError = error ?? (desktopAvailable ? subscriptionError : null);
  const inlineNativeNotice =
    !replacesNativePresentation &&
    !genericError &&
    (projection?.phase === "healthFailed" || projection?.phase === "exited")
      ? nativeNotice
      : null;
  const replacement =
    replacesNativePresentation && nativeNotice ? (
      <ResourceStateNotice {...nativeNotice} />
    ) : undefined;
  // `previewDesktop` rejects discovery responses above MAX_PREVIEW_TARGETS.
  // Keep the admitted, bounded target controls stable across phase revisions.
  const targetRows = useMemo(
    () =>
      targets.map((target) => {
        const selected =
          activeTarget?.rootId === target.rootId &&
          activeTarget.scriptName === target.scriptName;
        return (
          <div
            className="agent-preview-target"
            key={`${target.rootId}:${target.scriptName}`}
          >
            <span
              className="agent-preview-target__name"
              title={target.scriptName}
            >
              {target.scriptName}
            </span>
            {selected && projection?.phase === "active" ? (
              <button
                type="button"
                className="button button--compact button--danger"
                onClick={() =>
                  void stopPreviewTarget(target).catch(() => undefined)
                }
              >
                Stop
              </button>
            ) : (
              <button
                type="button"
                className="button button--compact button--primary"
                disabled={!desktopAvailable || !listenerReady || busy || !owner}
                onClick={() => {
                  if (owner)
                    void startPreviewTarget(target).catch(
                      (reason: unknown) =>
                        samePreviewOwner(owner, ownerRef.current) &&
                        setError(
                          reason instanceof Error
                            ? reason.message
                            : "Preview request failed safely.",
                        ),
                    );
                }}
              >
                Start
              </button>
            )}
          </div>
        );
      }),
    [
      activeTarget,
      busy,
      desktopAvailable,
      listenerReady,
      owner,
      projection?.phase,
      targets,
    ],
  );

  if (!visible || !container) return null;

  return createPortal(
    <div
      className="agent-preview-stack"
      data-preview-listener-ready={listenerReady ? "true" : "false"}
      data-preview-phase={projection?.phase ?? "none"}
      data-preview-target-identity={
        activeTarget
          ? `${activeTarget.rootId}:${activeTarget.scriptName}`
          : undefined
      }
    >
      {genericError ? (
        <ResourceStateNotice
          state="error"
          title="Preview is unavailable"
          detail={genericError}
        />
      ) : inlineNativeNotice ? (
        <ResourceStateNotice {...inlineNativeNotice} />
      ) : null}
      {targetRows.length > 0 ? (
        <div
          className="agent-preview-targets"
          role="group"
          aria-label="Preview targets"
        >
          {targetRows}
        </div>
      ) : null}
      <HarnessBrowserSurface owner={browseOwner} replacement={replacement} />
    </div>,
    container,
  );
}
