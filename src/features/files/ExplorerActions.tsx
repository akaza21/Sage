import { useWorkbenchStore } from "../../app/store";
import { selectOpenProject } from "../../app/workbenchSelectors";
import { ExplorerActionMenu } from "./ExplorerActionMenu";
import { NewFileControl, NewFolderControl } from "./NewFileControl";
import { explorerCreateParent } from "./newFileContract";

export function ExplorerActions() {
  const project = useWorkbenchStore(selectOpenProject);
  const selectedRootId = useWorkbenchStore((state) => state.selectedRootId);
  const selectedResource = useWorkbenchStore((state) => state.selectedResource);
  const tabs = useWorkbenchStore((state) => state.tabs);
  const activeTabId = useWorkbenchStore((state) => state.activeTabId);
  const listings = useWorkbenchStore((state) => state.directoryListings);
  const parent = project
    ? explorerCreateParent(
        project,
        selectedRootId,
        selectedResource,
        tabs,
        activeTabId,
        listings,
      )
    : null;
  const parentRootName =
    project?.roots.find((root) => root.id === parent?.rootId)?.name ?? null;
  return (
    <div className="sidebar__heading-actions">
      <NewFileControl parent={parent} rootName={parentRootName} />
      <NewFolderControl parent={parent} rootName={parentRootName} />
      <ExplorerActionMenu />
    </div>
  );
}
