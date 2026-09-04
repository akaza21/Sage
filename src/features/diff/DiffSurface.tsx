import { FileDiff } from "lucide-react";
import { useEffect } from "react";

import { useWorkbenchStore } from "../../app/store";
import { diffCacheKey } from "../../domain/diffIdentity";
import { sameResource } from "../../domain/resourceIdentity";
import type { DiffLineKind, WorkbenchTab } from "../../domain/workbench";
import { ResourceStateNotice } from "../../ui/ResourceStateNotice";

const linePrefix: Record<DiffLineKind, string> = {
  context: " ",
  added: "+",
  removed: "-",
  meta: "\\",
};

export function DiffSurface({ tab }: { tab: WorkbenchTab }) {
  const tabResource = tab.resource;
  const tabRepository = tab.repository;
  const path = tabResource?.relativePath ?? tab.title;
  const repositories = useWorkbenchStore((state) => state.repositories);
  const diff = useWorkbenchStore((state) =>
    tabResource
      ? state.diffs[diffCacheKey(tabRepository, tabResource)]
      : undefined,
  );
  const loadDiff = useWorkbenchStore((state) => state.loadDiff);
  const change = repositories.roots
    .flatMap((repository) => repository.changes)
    .find((candidate) => sameResource(candidate.resource, tabResource));

  useEffect(() => {
    if (tabResource) {
      void loadDiff(tabResource, tabRepository);
    }
  }, [loadDiff, tabResource, tabRepository]);

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
        <span>Working tree changes</span>
        <span className="diff-toolbar__hint">Read-only</span>
      </div>
      {!diff || diff.phase === "loading" ? (
        <ResourceStateNotice state="loading" title="Loading diff…" />
      ) : null}
      {diff?.phase === "error" ? (
        <ResourceStateNotice
          state="error"
          title="Could not load this diff"
          detail={diff.message}
        />
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
                  className={`diff-line diff-line--${line.kind}`}
                  role="row"
                  key={`${hunkIndex}:${lineIndex}`}
                >
                  <span>{line.oldLine}</span>
                  <span>{line.newLine}</span>
                  <code>
                    {linePrefix[line.kind]}
                    {line.text}
                  </code>
                </div>
              ))}
            </div>
          ))}
          {diff.value.hunks.length === 0 && !diff.value.binary ? (
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
