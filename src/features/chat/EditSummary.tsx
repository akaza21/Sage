import { ChevronRight, Files, GitBranch } from "lucide-react";

import { useWorkbenchStore } from "../../app/store";
import { resourceKey } from "../../domain/resourceIdentity";

export function EditSummary() {
  const changes = useWorkbenchStore((state) => state.turnChanges);
  const openDiff = useWorkbenchStore((state) => state.openDiff);
  const setSidebarView = useWorkbenchStore((state) => state.setSidebarView);

  if (changes.length === 0) {
    return null;
  }

  const additions = changes.reduce(
    (total, change) => total + (change.additions ?? 0),
    0,
  );
  const deletions = changes.reduce(
    (total, change) => total + (change.deletions ?? 0),
    0,
  );

  return (
    <section className="edit-summary" aria-label="Files edited in this turn">
      <div className="edit-summary__header">
        <span className="edit-summary__icon">
          <Files size={17} aria-hidden="true" />
        </span>
        <div>
          <strong>Edited {changes.length} files</strong>
          <div className="edit-summary__totals">
            <span>+{additions}</span>
            <span>-{deletions}</span>
          </div>
        </div>
        <button
          className="edit-summary__source-control"
          type="button"
          onClick={() => setSidebarView("sourceControl")}
        >
          <GitBranch size={14} aria-hidden="true" />
          Source Control
        </button>
      </div>
      <div className="edit-summary__files">
        {changes.map((change) => (
          <button
            className="edit-file"
            key={resourceKey(change.resource)}
            type="button"
            aria-label={`Open diff for ${change.resource.relativePath}`}
            onClick={() => openDiff(change.resource)}
          >
            <span className="edit-file__path">
              {change.resource.relativePath}
            </span>
            <span className="edit-file__counts">
              <span>
                {change.additions === null ? "—" : `+${change.additions}`}
              </span>
              <span>
                {change.deletions === null ? "—" : `-${change.deletions}`}
              </span>
            </span>
            <ChevronRight size={14} aria-hidden="true" />
          </button>
        ))}
      </div>
    </section>
  );
}
