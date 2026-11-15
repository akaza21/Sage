import type { ResourceState } from "../../app/store";
import type { PathGlobMatcher } from "../../domain/pathGlobMatcher";
import { resourceKey } from "../../domain/resourceIdentity";
import type {
  DirectoryListing,
  ResourceRef,
  WorkspaceEntry,
} from "../../domain/workbench";

export const EXPLORER_ROW_LIMIT = 500;

export interface VisibleWorkspaceEntry extends WorkspaceEntry {
  depth: number;
}

export interface FlattenedFileTree {
  entries: VisibleWorkspaceEntry[];
  truncated: boolean;
}

export function flattenFileTree(
  root: ResourceRef,
  listings: Record<string, ResourceState<DirectoryListing>>,
  expanded: Record<string, boolean>,
  limit = EXPLORER_ROW_LIMIT,
  hideIgnored = false,
  excluded?: PathGlobMatcher,
): FlattenedFileTree {
  const entries: VisibleWorkspaceEntry[] = [];
  let truncated = false;

  function appendDirectory(resource: ResourceRef, depth: number) {
    const listing = listings[resourceKey(resource)];
    if (listing?.phase !== "ready") {
      return;
    }
    truncated ||= listing.value.truncated;
    for (const entry of listing.value.entries) {
      if (hideIgnored && entry.ignored) continue;
      if (excluded?.(entry.resource.relativePath)) continue;
      if (entries.length === limit) {
        truncated = true;
        return;
      }
      entries.push({ ...entry, depth });
      if (entry.kind === "directory" && expanded[resourceKey(entry.resource)]) {
        appendDirectory(entry.resource, depth + 1);
      }
    }
  }

  appendDirectory(root, 0);
  return { entries, truncated };
}
