import { useWorkbenchStore } from "../../app/store";
import { selectOpenProject } from "../../app/workbenchSelectors";
import {
  parentResource,
  resourceKey,
  sameResource,
} from "../../domain/resourceIdentity";
import type { TrashResourceKind } from "../../domain/workbench";
import { NewFileControl, NewFolderControl } from "./NewFileControl";
import { explorerCreateParent } from "./newFileContract";
import { RenameFileControl } from "./RenameFileControl";
import { TrashResourceControl } from "./TrashResourceControl";

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
  const selectedParent = selectedResource
    ? parentResource(selectedResource)
    : null;
  const selectedListing = selectedParent
    ? listings[resourceKey(selectedParent)]
    : null;
  const selectedEntry =
    selectedListing?.phase === "ready" && !selectedListing.value.truncated
      ? selectedListing.value.entries.find((entry) =>
          sameResource(entry.resource, selectedResource),
        )
      : null;
  const renameSource =
    selectedEntry?.kind === "file" ? selectedEntry.resource : null;
  const selectedFolderListing = selectedResource
    ? listings[resourceKey(selectedResource)]
    : null;
  const trashKind: TrashResourceKind | null =
    selectedEntry?.kind === "file"
      ? "file"
      : selectedEntry?.kind === "directory" &&
          selectedFolderListing?.phase === "ready" &&
          !selectedFolderListing.value.truncated &&
          selectedFolderListing.value.entries.length === 0
        ? "emptyFolder"
        : null;
  const trashSource = trashKind ? (selectedEntry?.resource ?? null) : null;
  const selectedRootName =
    project?.roots.find((root) => root.id === selectedResource?.rootId)?.name ??
    null;

  return (
    <span className="sidebar__heading-actions">
      <span className="muted">{project?.name ?? "No project"}</span>
      <NewFileControl parent={parent} rootName={parentRootName} />
      <NewFolderControl parent={parent} rootName={parentRootName} />
      <RenameFileControl source={renameSource} rootName={selectedRootName} />
      <TrashResourceControl
        source={trashSource}
        kind={trashKind}
        rootName={selectedRootName}
      />
    </span>
  );
}
