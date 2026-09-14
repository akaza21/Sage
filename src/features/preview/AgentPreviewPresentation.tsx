import { useMemo } from "react";

import type {
  PreviewProjection,
  PreviewProjectionOwner,
} from "../../domain/previewProjection";
import { ResourceStateNotice } from "../../ui/ResourceStateNotice";
import { HarnessBrowserSurface } from "./HarnessBrowserSurface";
import type { HarnessBrowseOwner } from "./harnessBrowse";
import { PreviewLifecycleStatus } from "./PreviewLifecycleStatus";
import { previewOrigin } from "./previewPresentation";
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
  const phase = projection?.phase ?? null;
  const activeTarget = projection?.target ?? null;
  const replacesNativePresentation = Boolean(
    projection &&
    projection.phase !== "active" &&
    (!session ||
      (lastAdmittedNativeOrigin &&
        previewOrigin(session.href) === lastAdmittedNativeOrigin)),
  );
  const showInlineLifecycle = Boolean(
    !replacesNativePresentation &&
    !error &&
    projection &&
    projection.phase !== "idle",
  );
  const replacement =
    replacesNativePresentation && projection ? (
      <PreviewLifecycleStatus
        failure={projection.failure}
        phase={projection.phase}
      />
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
            <span className="agent-preview-target__identity">
              <span
                className="agent-preview-target__name"
                title={target.scriptName}
              >
                {target.scriptName}
              </span>
              {selected && phase ? (
                <span className="agent-preview-target__state">
                  {phase === "active" ? "Running" : "Selected"}
                </span>
              ) : null}
            </span>
            {selected && phase === "active" ? (
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
      phase,
      targets,
    ],
  );

  return (
    <div
      className="agent-preview-stack"
      data-preview-listener-ready={listenerReady ? "true" : "false"}
      data-preview-phase={phase ?? "none"}
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
      ) : showInlineLifecycle && projection ? (
        <PreviewLifecycleStatus
          compact
          failure={projection.failure}
          phase={projection.phase}
        />
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
