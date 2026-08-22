import { useWorkbenchStore } from "../../app/store";
import { WorkspaceSearchView } from "./WorkspaceSearchView";

export function SearchSurface() {
  const projectId = useWorkbenchStore((state) => state.project?.id ?? null);
  const transitionEpoch = useWorkbenchStore(
    (state) => state.nativeTransitionEpoch,
  );
  const openSearchMatch = useWorkbenchStore((state) => state.openSearchMatch);

  return (
    <WorkspaceSearchView
      projectId={projectId}
      transitionEpoch={transitionEpoch}
      onOpenMatch={(request) => void openSearchMatch(request)}
    />
  );
}
