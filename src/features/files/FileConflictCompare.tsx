import { useEffect, useState } from "react";

import { errorMessage } from "../../domain/errors";
import type { FileDiff, ResourceRef } from "../../domain/workbench";
import { getWorkbenchPorts } from "../../ports/workbenchRuntime";
import { ResourceStateNotice } from "../../ui/ResourceStateNotice";
import { DiffHunks } from "../diff/DiffHunks";
import {
  MAX_SNAPSHOT_DISPLAY_LINES,
  snapshotFileDiff,
} from "../diff/snapshotDiff";

type CompareState =
  | { phase: "loading" }
  | { phase: "ready"; diff: FileDiff }
  | { phase: "error"; message: string };

/**
 * The File on disk against the unsaved buffer, drawn with the Diff surface's
 * own hunks. It reads disk once per buffer and keeps only the bounded hunks.
 */
export function FileConflictCompare({
  resource,
  text,
}: {
  resource: ResourceRef;
  text: string;
}) {
  const [state, setState] = useState<CompareState>({ phase: "loading" });
  useEffect(() => {
    // The surface remounts this per File, so it starts at loading.
    let cancelled = false;
    getWorkbenchPorts()
      .editor.readTextFile(resource)
      .then((file) => {
        const disk = file.content.replace(/\r\n?/gu, "\n");
        return {
          phase: "ready",
          diff: snapshotFileDiff(resource, disk, text),
        } as const;
      })
      .catch((error: unknown) => ({
        phase: "error" as const,
        message: errorMessage(error),
      }))
      .then((next) => {
        if (!cancelled) setState(next);
      });
    return () => {
      cancelled = true;
    };
  }, [resource, text]);

  return (
    <section
      className="diff-surface file-conflict-compare"
      aria-label={`Compare ${resource.relativePath} on disk with your version`}
    >
      <div className="diff-toolbar">
        <span className="diff-toolbar__scope">
          On disk (−) against your unsaved version (+)
        </span>
        <span className="diff-toolbar__hint">Read-only</span>
      </div>
      {state.phase === "loading" ? (
        <ResourceStateNotice state="loading" title="Comparing…" />
      ) : null}
      {state.phase === "error" ? (
        <ResourceStateNotice
          state="error"
          title="Could not compare this file"
          detail={state.message}
        />
      ) : null}
      {state.phase === "ready" && state.diff.hunks.length === 0 ? (
        <ResourceStateNotice
          state="idle"
          title="Your version matches the file on disk."
        />
      ) : null}
      {state.phase === "ready" && state.diff.hunks.length > 0 ? (
        <DiffHunks
          diff={state.diff}
          label="Disk and unsaved version"
          maxLines={MAX_SNAPSHOT_DISPLAY_LINES}
          limitMessage="Only the first changes are shown."
        />
      ) : null}
    </section>
  );
}
