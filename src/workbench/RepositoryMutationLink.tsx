import { useWorkbenchStore } from "../app/store";
import type { RepositoryMutationError } from "../app/repositoryMutationSlice";
import { isRootRepository } from "../domain/repositoryInventory";
import { resourceKey } from "../domain/resourceIdentity";
import { Notice } from "../ui/Notice";

/** Source Control owns the explanation; the editor only points there. */
export default function RepositoryMutationLink({
  error,
}: {
  error: RepositoryMutationError;
}) {
  return (
    <Notice
      identity={`repository-mutation-link:${resourceKey(error.resource)}:${error.action}`}
      severity="warning"
      message="Source Control has a problem with this file."
      action={{
        label: "Show in Source Control",
        onClick: () => {
          const state = useWorkbenchStore.getState();
          state.setSidebarView("sourceControl");
          const selected = state.selectedRepository;
          if (
            selected &&
            !(
              selected.rootId === error.resource.rootId &&
              isRootRepository(selected)
            )
          ) {
            void state.selectRepository({
              rootId: error.resource.rootId,
              relativePath: "",
            });
          }
        },
      }}
    />
  );
}
