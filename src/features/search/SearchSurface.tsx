import { useWorkbenchStore } from "../../app/store";
import { WorkspaceSearchView } from "./WorkspaceSearchView";

export default function SearchSurface() {
  const projectId = useWorkbenchStore((state) => state.project?.id ?? null);
  const transitionEpoch = useWorkbenchStore(
    (state) => state.nativeTransitionEpoch,
  );
  const projectGeneration = useWorkbenchStore(
    (state) => state.projectGeneration,
  );
  const openFileLocation = useWorkbenchStore((state) => state.openFileLocation);

  return (
    <WorkspaceSearchView
      projectId={projectId}
      transitionEpoch={transitionEpoch}
      indexGeneration={projectGeneration}
      onOpenMatch={(request) =>
        void openFileLocation({
          projectId: request.projectId,
          transitionEpoch: request.transitionEpoch,
          resource: request.resource,
          lineNumber: request.lineNumber,
          columnNumber: request.columnNumber,
          selection: {
            kind: "match",
            query: request.query,
            caseSensitive: request.caseSensitive,
          },
        })
      }
    />
  );
}
