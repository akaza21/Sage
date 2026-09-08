import { useMemo } from "react";

import type {
  PreviewProjection,
  PreviewProjectionOwner,
} from "../../domain/previewProjection";
import { ResourceStateNotice } from "../../ui/ResourceStateNotice";
import { HarnessBrowserSurface } from "./HarnessBrowserSurface";
import type { HarnessBrowseOwner } from "./harnessBrowse";
import { previewNotice, previewOrigin } from "./previewPresentation";
import type { PreviewTargetRef } from "./previewDesktop";
import { useHarnessBrowseSession } from "./useHarnessBrowseSession";

export function AgentPreviewPresentation({
  owner,
  browseOwner,
  desktopAvailable,
  listenerReady,
  projection,
  targets,
  error,
  lastAdmittedNativeOrigin,
  onStart,
  onStop,
}: {
  owner: PreviewProjectionOwner | null;
  browseOwner: HarnessBrowseOwner | null;
  desktopAvailable: boolean;
  listenerReady: boolean;
  projection: PreviewProjection | null;
  targets: PreviewTargetRef[];
  error: string | null;
  lastAdmittedNativeOrigin: string | null;
  onStart: (target: PreviewTargetRef) => void;
  onStop: (target: PreviewTargetRef) => void;
}) {
  const session = useHarnessBrowseSession(browseOwner);
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
  const inlineNativeNotice =
    !replacesNativePresentation &&
    !error &&
    (projection?.phase === "healthFailed" || projection?.phase === "exited")
      ? nativeNotice
      : null;
  const replacement =
    replacesNativePresentation && nativeNotice ? (
      <ResourceStateNotice {...nativeNotice} />
    ) : undefined;
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
                onClick={() => onStop(target)}
              >
                Stop
              </button>
            ) : (
              <button
                type="button"
                className="button button--compact button--primary"
                disabled={!desktopAvailable || !listenerReady || busy || !owner}
                onClick={() => onStart(target)}
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
      onStart,
      onStop,
      owner,
      projection?.phase,
      targets,
    ],
  );

  return (
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
      {error ? (
        <ResourceStateNotice
          state="error"
          title="Preview is unavailable"
          detail={error}
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
    </div>
  );
}
