import { ChevronRight } from "../../ui/leanIcons";
import { forwardRef, memo } from "react";

import { resourceKey } from "../../domain/resourceIdentity";
import type { ResourceRef } from "../../domain/workbench";
import { FileKindIcon } from "./FileKindIcon";
import type { VisibleWorkspaceEntry } from "./fileTree";

interface ExplorerRowProps {
  entry: VisibleWorkspaceEntry;
  selected: boolean;
  expanded: boolean;
  onSelectDirectory: (resource: ResourceRef) => void;
  onToggleDirectory: (resource: ResourceRef) => void;
  onOpenFile: (resource: ResourceRef) => Promise<void>;
  /** A right-click, or the context-menu key, on the row (ED-18). */
  onContextMenu?: (resource: ResourceRef, row: HTMLButtonElement) => boolean;
}

export const ExplorerRow = memo(
  forwardRef<HTMLButtonElement, ExplorerRowProps>(function ExplorerRow(
    {
      entry,
      selected,
      expanded,
      onSelectDirectory,
      onToggleDirectory,
      onOpenFile,
      onContextMenu,
    },
    ref,
  ) {
    const key = resourceKey(entry.resource);
    const directory = entry.kind === "directory";
    const file = entry.kind === "file";
    const entryKind = directory ? "directory" : file ? "file" : "symlink";

    return (
      <button
        className={`tree__row ${directory ? "tree__row--folder" : ""} ${
          selected ? "tree__row--selected" : ""
        } ${entry.ignored ? "tree__row--ignored" : ""}`}
        data-resource-key={key}
        ref={ref}
        type="button"
        role="treeitem"
        aria-label={
          directory
            ? `Toggle folder ${entry.resource.relativePath}`
            : file
              ? `Open file ${entry.resource.relativePath}`
              : `Unavailable resource ${entry.resource.relativePath}`
        }
        aria-current={selected ? "page" : undefined}
        aria-expanded={directory ? expanded : undefined}
        title={entry.resource.relativePath}
        style={{ paddingLeft: 8 + entry.depth * 16 }}
        disabled={!directory && !file}
        onClick={() => {
          if (directory) {
            onSelectDirectory(entry.resource);
            onToggleDirectory(entry.resource);
          } else {
            void onOpenFile(entry.resource);
          }
        }}
        onContextMenu={(event) => {
          if (onContextMenu?.(entry.resource, event.currentTarget)) {
            event.preventDefault();
          }
        }}
      >
        {directory ? (
          <ChevronRight
            className={expanded ? "tree__chevron--expanded" : undefined}
            size={14}
            aria-hidden="true"
          />
        ) : (
          <span className="tree__indent" />
        )}
        <FileKindIcon
          path={entry.resource.relativePath}
          entryKind={entryKind}
        />
        <span>{entry.name}</span>
      </button>
    );
  }),
);
