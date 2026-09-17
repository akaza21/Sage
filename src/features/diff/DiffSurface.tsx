import { FileDiff } from "lucide-react";
import { useEffect } from "react";

import { usePinnedAgentChatOwner } from "../../app/agentChatOwner";
import { useWorkbenchStore } from "../../app/store";
import { acpProjectionStoreFor } from "../../app/acpProjectionRegistry";
import { snapshotProjectionRevision } from "../../app/agentDiffAdmission";
import { diffCacheKey } from "../../domain/diffIdentity";
import { isSnapshotDiffTab } from "../../domain/agentDiffIdentity";
import { sameResource } from "../../domain/resourceIdentity";
import type { DiffLineCommentContext } from "../../domain/attachedContext";
import type { DiffLineKind, WorkbenchTab } from "../../domain/workbench";
import { ResourceStateNotice } from "../../ui/ResourceStateNotice";
import { DiffLineCommentComposer } from "./DiffLineCommentComposer";

const linePrefix: Record<DiffLineKind, string> = {
  context: " ",
  added: "+",
  removed: "-",
  meta: "\\",
};

export function DiffSurface({
  tab,
  readOnly = false,
}: {
  tab: WorkbenchTab;
  readOnly?: boolean;
}) {
  return tab.diffRef ? (
    <SnapshotDiffSurface tab={tab} commentEnabled={readOnly} />
  ) : (
    <DiffDocument tab={tab} unavailable={isSnapshotDiffTab(tab)} />
  );
}

function SnapshotDiffSurface({
  tab,
  commentEnabled,
}: {
  tab: WorkbenchTab;
  commentEnabled: boolean;
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
    />
  );
}

function DiffDocument({
  tab,
  unavailable = false,
  commentEnabled = false,
}: {
  tab: WorkbenchTab;
  unavailable?: boolean;
  commentEnabled?: boolean;
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
        message: "This saved edit is unavailable for the current Chat session.",
      }
    : cached;
  const loadDiff = useWorkbenchStore((state) => state.loadDiff);
  const change = isSnapshotDiffTab(tab)
    ? undefined
    : repositories.roots
        .flatMap((repository) => repository.changes)
        .find((candidate) => sameResource(candidate.resource, tabResource));

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
      <header className="diff-header">
        <div className="diff-header__path">
          <FileDiff size={16} aria-hidden="true" />
          <span>{path}</span>
        </div>
        <div className="diff-header__counts">
          <span>{change?.additions == null ? "" : `+${change.additions}`}</span>
          <span>{change?.deletions == null ? "" : `-${change.deletions}`}</span>
        </div>
      </header>
      <div className="diff-toolbar">
        <span>
          {isSnapshotDiffTab(tab)
            ? "Saved agent change"
            : "Working tree changes"}
        </span>
        <span className="diff-toolbar__hint">Read-only</span>
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
        <div
          className="diff-editor"
          role="table"
          aria-label="Unified file diff"
        >
          {diff.value.binary ? (
            <p className="resource-state">Binary file changed.</p>
          ) : null}
          {diff.value.hunks.map((hunk, hunkIndex) => (
            <div className="diff-hunk" key={`${hunk.header}:${hunkIndex}`}>
              <div className="diff-hunk__header" role="row">
                {hunk.header}
              </div>
              {hunk.lines.map((line, lineIndex) => (
                <div
                  className={`diff-line diff-line--${line.kind}${canComment ? " diff-line--commentable" : ""}`}
                  role="row"
                  key={`${hunkIndex}:${lineIndex}`}
                >
                  <span role="cell">{line.oldLine}</span>
                  <span role="cell">{line.newLine}</span>
                  <div className="diff-line__comment-gutter" role="cell">
                    {canComment &&
                    tab.diffRef &&
                    (line.newLine ?? line.oldLine) !== null &&
                    line.kind !== "meta" ? (
                      <DiffLineCommentComposer
                        diffRef={tab.diffRef}
                        side={line.newLine !== null ? "new" : "old"}
                        line={(line.newLine ?? line.oldLine)!}
                        onAdd={addContext}
                      />
                    ) : null}
                  </div>
                  <code role="cell">
                    {linePrefix[line.kind]}
                    {line.text}
                  </code>
                </div>
              ))}
            </div>
          ))}
          {diff.value.hunks.length === 0 &&
          !diff.value.binary &&
          !diff.value.truncated ? (
            <p className="resource-state">No textual changes for this file.</p>
          ) : null}
          {diff.value.truncated ? (
            <p className="diff-limit">
              Diff capped at 1,200 lines to protect UI responsiveness.
            </p>
          ) : null}
        </div>
      ) : null}
    </section>
  );
}
