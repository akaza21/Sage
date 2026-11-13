import { FileDiff, FileText, RefreshCw } from "../../ui/leanIcons";
import { useEffect } from "react";

import { usePinnedAgentChatOwner } from "../../app/agentChatOwner";
import { useWorkbenchStore } from "../../app/store";
import { acpProjectionStoreFor } from "../../app/acpProjectionRegistry";
import { snapshotProjectionRevision } from "../../app/snapshotProjection";
import { diffCacheKey } from "../../domain/diffIdentity";
import { isSnapshotDiffTab } from "../../domain/agentDiffIdentity";
import { sameResource } from "../../domain/resourceIdentity";
import type { DiffLineCommentContext } from "../../domain/attachedContext";
import type { WorkbenchTab } from "../../domain/workbench";
import type { CanvasSurfaceOptions } from "../../workbench/surfaceRegistry";
import { ResourceStateNotice } from "../../ui/ResourceStateNotice";
import { DiffHunks } from "./DiffHunks";
import { DiffLineCommentComposer } from "./DiffLineCommentComposer";

export function DiffSurface({
  tab,
  readOnly = false,
  hostHeader = false,
}: {
  tab: WorkbenchTab;
} & CanvasSurfaceOptions) {
  return tab.diffRef ? (
    <SnapshotDiffSurface
      tab={tab}
      commentEnabled={readOnly}
      hostHeader={hostHeader}
    />
  ) : (
    <DiffDocument
      tab={tab}
      unavailable={isSnapshotDiffTab(tab)}
      workingTreeNotice={readOnly && !isSnapshotDiffTab(tab)}
      hostHeader={hostHeader}
    />
  );
}

function SnapshotDiffSurface({
  tab,
  commentEnabled,
  hostHeader,
}: {
  tab: WorkbenchTab;
  commentEnabled: boolean;
  hostHeader: boolean;
}) {
  const project = useWorkbenchStore((state) => state.project);
  const projectGeneration = useWorkbenchStore(
    (state) => state.projectGeneration,
  );
  const projectTransitioning = useWorkbenchStore(
    (state) => state.projectTransitioning,
  );
  const ref = tab.diffRef!;
  const current = acpProjectionStoreFor(ref.chatId)(
    (projection) =>
      snapshotProjectionRevision(
        { project, projectGeneration, projectTransitioning },
        ref,
        projection,
      ) !== null,
  );
  return (
    <DiffDocument
      tab={tab}
      unavailable={!current}
      commentEnabled={commentEnabled && current}
      hostHeader={hostHeader}
    />
  );
}

function DiffDocument({
  tab,
  unavailable = false,
  commentEnabled = false,
  workingTreeNotice = false,
  hostHeader = false,
}: {
  tab: WorkbenchTab;
  unavailable?: boolean;
  commentEnabled?: boolean;
  workingTreeNotice?: boolean;
  hostHeader?: boolean;
}) {
  const focusedChatId = usePinnedAgentChatOwner();
  const captureChatOwnership = useWorkbenchStore(
    (state) => state.captureChatOwnership,
  );
  const attachChatContext = useWorkbenchStore(
    (state) => state.attachChatContext,
  );
  const commentBusy = useWorkbenchStore((state) =>
    focusedChatId ? (state.chatUiById[focusedChatId]?.sending ?? false) : true,
  );
  const tabResource = tab.resource;
  const tabRepository = tab.repository;
  const path = tabResource?.relativePath ?? tab.title;
  const repositories = useWorkbenchStore((state) => state.repositories);
  const cached = useWorkbenchStore((state) =>
    tabResource
      ? state.diffs[diffCacheKey(tabRepository, tabResource, tab.diffRef)]
      : undefined,
  );
  const diff = unavailable
    ? {
        phase: "error" as const,
        message: "This saved edit is unavailable for the current chat session.",
      }
    : cached;
  const loadDiff = useWorkbenchStore((state) => state.loadDiff);
  const openFile = useWorkbenchStore((state) => state.openFile);
  const change = isSnapshotDiffTab(tab)
    ? undefined
    : repositories.roots
        .flatMap((repository) => repository.changes)
        .find((candidate) => sameResource(candidate.resource, tabResource));
  const visibleCounts =
    diff?.phase === "ready" && !diff.value.binary && !diff.value.truncated
      ? diff.value.hunks.reduce(
          (counts, hunk) => {
            for (const line of hunk.lines) {
              if (line.kind === "added") counts.additions += 1;
              if (line.kind === "removed") counts.deletions += 1;
            }
            return counts;
          },
          { additions: 0, deletions: 0 },
        )
      : null;
  // A complete diff is what the body shows, so its counts win over Git status,
  // which can be older than the diff or come from a fixture with no hunks.
  const additions = visibleCounts?.additions ?? change?.additions;
  const deletions = visibleCounts?.deletions ?? change?.deletions;

  const counts = (
    <div className="diff-header__counts">
      <span>{additions == null ? "" : `+${additions}`}</span>
      <span>{deletions == null ? "" : `-${deletions}`}</span>
    </div>
  );

  const canComment =
    commentEnabled &&
    !commentBusy &&
    Boolean(tab.diffRef) &&
    focusedChatId === tab.diffRef?.chatId;
  const addContext = (context: DiffLineCommentContext) => {
    if (!canComment || !focusedChatId) return false;
    const intent = captureChatOwnership(focusedChatId);
    return intent ? attachChatContext(intent, context) : false;
  };

  const refreshDiff = () => {
    if (!tabResource || unavailable || !diff || diff.phase === "loading")
      return;
    const key = diffCacheKey(tabRepository, tabResource, tab.diffRef);
    useWorkbenchStore.setState((state) => {
      const diffs = { ...state.diffs };
      delete diffs[key];
      return { diffs };
    });
    void loadDiff(tabResource, tabRepository, tab.diffRef);
  };

  useEffect(() => {
    if (tabResource && !unavailable) {
      if (tab.diffRef) void loadDiff(tabResource, tabRepository, tab.diffRef);
      else void loadDiff(tabResource, tabRepository);
    }
  }, [loadDiff, tabResource, tabRepository, tab.diffRef, unavailable]);

  return (
    <section
      className="diff-surface resource-document"
      aria-label={`Diff for ${path}`}
    >
      {workingTreeNotice ? (
        <p role="status">
          Current working-tree changes against Git; may include edits from other
          turns.
        </p>
      ) : null}
      {hostHeader ? null : (
        <header className="diff-header">
          <div className="diff-header__path">
            <FileDiff size={16} aria-hidden="true" />
            <span>{path}</span>
          </div>
          {counts}
        </header>
      )}
      <div className="diff-toolbar">
        <span className="diff-toolbar__scope">
          {isSnapshotDiffTab(tab)
            ? "Saved agent change"
            : "Changes since HEAD (or initial tree)"}
        </span>
        {hostHeader ? (
          counts
        ) : (
          <span className="diff-toolbar__hint">Read-only</span>
        )}
        <div
          className="diff-toolbar__actions"
          role="group"
          aria-label="Diff actions"
        >
          {tabResource && !unavailable ? (
            <button
              type="button"
              aria-label="Refresh diff"
              title="Refresh diff"
              disabled={!diff || diff.phase === "loading"}
              onClick={refreshDiff}
            >
              <RefreshCw size={14} aria-hidden="true" />
            </button>
          ) : null}
          {tabResource && !isSnapshotDiffTab(tab) ? (
            <button
              type="button"
              aria-label="Open file"
              title="Open file"
              onClick={() => void openFile(tabResource)}
            >
              <FileText size={14} aria-hidden="true" />
            </button>
          ) : null}
        </div>
      </div>
      {!diff || diff.phase === "loading" ? (
        <ResourceStateNotice state="loading" title="Loading diff…" />
      ) : null}
      {diff?.phase === "error" ? (
        <>
          <ResourceStateNotice
            state="error"
            title="Could not load this diff"
            detail={diff.message}
          />
          {tab.diffRef && tabResource && !unavailable ? (
            <button
              type="button"
              onClick={() => void loadDiff(tabResource, undefined, tab.diffRef)}
            >
              Retry saved diff
            </button>
          ) : null}
        </>
      ) : null}
      {diff?.phase === "ready" ? (
        <>
          <DiffHunks
            diff={diff.value}
            label="Unified file diff"
            maxLines={1200}
            limitMessage="Diff capped at 1,200 lines to protect UI responsiveness."
            renderComment={
              canComment && tab.diffRef
                ? (line) =>
                    (line.newLine ?? line.oldLine) !== null &&
                    line.kind !== "meta" ? (
                      <DiffLineCommentComposer
                        diffRef={tab.diffRef!}
                        side={line.newLine !== null ? "new" : "old"}
                        line={(line.newLine ?? line.oldLine)!}
                        onAdd={addContext}
                      />
                    ) : null
                : undefined
            }
          />
        </>
      ) : null}
    </section>
  );
}
