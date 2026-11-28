import { memo } from "react";

import { repositoryKey, sameRepository } from "../domain/repositoryInventory";
import type { RepositoryRef, RepositorySummary } from "../domain/workbench";

const MAX_SHOWN_CHANGES = 300;

function changeCountLabel(summary: RepositorySummary): string {
  return summary.changesOverflowed || summary.changeCount > MAX_SHOWN_CHANGES
    ? `${MAX_SHOWN_CHANGES}+`
    : String(summary.changeCount);
}

function rowName(
  ref: RepositoryRef,
  summary: RepositorySummary | undefined,
): string {
  const name = ref.relativePath || "Repository root";
  if (!summary) return name;
  if (summary.state !== "repository") {
    return `${name}, unavailable`;
  }
  const count = changeCountLabel(summary);
  const changes = count === "1" ? "1 change" : `${count} changes`;
  return `${name}, ${summary.branch ?? "no branch"}, ${changes}`;
}

/**
 * The repositories found directly inside a folder that is not itself a Git
 * repository. Selecting one shows its changes below the list.
 */
export const SourceRepositoryList = memo(function SourceRepositoryList({
  refs,
  summaries,
  selected,
  truncated,
  disabled,
  onSelect,
}: {
  refs: readonly RepositoryRef[];
  summaries: readonly RepositorySummary[];
  selected: RepositoryRef | null;
  truncated: boolean;
  disabled: boolean;
  onSelect: (repository: RepositoryRef) => void;
}) {
  const byKey = new Map(
    summaries.map((summary) => [repositoryKey(summary.ref), summary]),
  );
  return (
    <div className="source-repositories">
      <ul className="source-repositories__list" aria-label="Repositories">
        {refs.map((ref) => {
          const summary = byKey.get(repositoryKey(ref));
          const current = sameRepository(selected, ref);
          return (
            <li key={repositoryKey(ref)}>
              <button
                type="button"
                className="source-repositories__row"
                aria-label={rowName(ref, summary)}
                aria-current={current ? "true" : undefined}
                title={summary?.message ?? undefined}
                disabled={disabled}
                onClick={() => {
                  if (!current) onSelect(ref);
                }}
              >
                <span className="source-repositories__name">
                  {ref.relativePath || "Repository root"}
                </span>
                {summary?.state === "repository" ? (
                  <>
                    <span className="source-repositories__branch">
                      {summary.branch ?? ""}
                    </span>
                    <span className="source-repositories__count">
                      {changeCountLabel(summary)}
                    </span>
                  </>
                ) : summary ? (
                  <span className="source-repositories__branch">
                    Unavailable
                  </span>
                ) : null}
              </button>
            </li>
          );
        })}
      </ul>
      {truncated ? (
        <p className="sidebar__notice">More repositories not shown.</p>
      ) : null}
    </div>
  );
});
