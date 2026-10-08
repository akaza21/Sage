import { Files, ChevronRight } from "../../ui/leanIcons";

import { useWorkbenchStore } from "../../app/store";
import { resourceKey } from "../../domain/resourceIdentity";

export function EditSummary() {
  const changes = useWorkbenchStore((state) => state.turnChanges);
  const openDiff = useWorkbenchStore((state) => state.openDiff);

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
          <Files size={14} aria-hidden="true" />
        </span>
        <strong>
          Edited {changes.length} {changes.length === 1 ? "file" : "files"}
        </strong>
        <span className="edit-summary__totals" aria-label="Line changes">
          <span>+{additions}</span>
          <span>-{deletions}</span>
        </span>
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
