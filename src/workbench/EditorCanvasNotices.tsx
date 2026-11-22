import { useWorkbenchStore } from "../app/store";
import type { RepositoryMutationError } from "../app/repositoryMutationSlice";
import { resourceKey } from "../domain/resourceIdentity";
import { SaveAllNotice } from "../features/files/SaveAllNotice";
import { Notice } from "../ui/Notice";

/**
 * Editor-wide notices, loaded only when Save All fails or a Git file action
 * fails on the active File or Diff. Source Control owns the Git explanation;
 * the editor only points there.
 */
export default function EditorCanvasNotices({
  mutationError,
  saveAllFailed,
}: {
  mutationError: RepositoryMutationError | null;
  saveAllFailed: boolean;
}) {
  return (
    <>
      {mutationError ? (
        <Notice
          identity={`repository-mutation-link:${resourceKey(mutationError.resource)}:${mutationError.action}`}
          severity="warning"
          message="Source Control has a problem with this file."
          action={{
            label: "Show in Source Control",
            // Source Control shows the failure even when its repository is
            // filtered out, so opening the view is enough.
            onClick: () =>
              useWorkbenchStore.getState().setSidebarView("sourceControl"),
          }}
        />
      ) : null}
      {saveAllFailed ? <SaveAllNotice /> : null}
    </>
  );
}
