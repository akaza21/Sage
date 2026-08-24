import { useWorkbenchStore } from "../../app/store";
import { WorkspaceSearchView } from "./WorkspaceSearchView";

export function SearchSurface() {
  const projectId = useWorkbenchStore((state) => state.project?.id ?? null);
  const transitionEpoch = useWorkbenchStore(
    (state) => state.nativeTransitionEpoch,
  );
  const projectGeneration = useWorkbenchStore(
    (state) => state.projectGeneration,
  );
  const openSearchMatch = useWorkbenchStore((state) => state.openSearchMatch);

  return (
    <WorkspaceSearchView
      projectId={projectId}
      transitionEpoch={transitionEpoch}
      indexGeneration={projectGeneration}
      onOpenMatch={(request) => void openSearchMatch(request)}
    />
  );
}
