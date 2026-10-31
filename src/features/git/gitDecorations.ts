import { isConflictedChange } from "../../domain/gitMutation";
import type {
  FileChange,
  ProjectRepositoryStatus,
  RootRepositoryStatus,
} from "../../domain/repository";
import type { ResourceRef } from "../../domain/workbench";
import { visibleChangeStatus } from "./conflictPresentation";

/**
 * Git decorations for the Explorer and tabs (EP-13), as in VS Code: a coloured
 * name and a letter for each changed File, and a tint and a dot for each
 * folder that holds one.
 */
export type GitDecorationTone =
  "conflict" | "modified" | "deleted" | "added" | "untracked" | "unknown";

export interface GitDecoration {
  tone: GitDecorationTone;
  /** The badge: a status letter, a folder's dot, or a "many changes" count. */
  badge: string;
  /** What the badge means, for the tooltip and screen readers. */
  label: string;
}

export interface RootGitDecorations {
  files: ReadonlyMap<string, GitDecoration>;
  /** Folders that hold a change, by relative path; `""` is the root. */
  folders: ReadonlyMap<string, GitDecoration>;
}

// One object per kind, so a row whose mark did not change gets the same prop
// after a refresh and its memo holds.
function decoration(
  tone: GitDecorationTone,
  badge: string,
  label: string,
): GitDecoration {
  return Object.freeze({ tone, badge, label });
}

const FILE_DECORATIONS = {
  conflict: decoration("conflict", "!", "Conflict"),
  modified: decoration("modified", "M", "Modified"),
  typeChanged: decoration("modified", "T", "Type changed"),
  deleted: decoration("deleted", "D", "Deleted"),
  added: decoration("added", "A", "Added"),
  renamed: decoration("added", "R", "Renamed"),
  copied: decoration("added", "C", "Copied"),
  untracked: decoration("untracked", "U", "Untracked"),
} as const;

const FOLDER_DECORATIONS: Record<GitDecorationTone, GitDecoration> = {
  conflict: decoration("conflict", "•", "Contains conflicts"),
  modified: decoration("modified", "•", "Contains changes"),
  deleted: decoration("deleted", "•", "Contains changes"),
  added: decoration("added", "•", "Contains changes"),
  untracked: decoration("untracked", "•", "Contains changes"),
  unknown: decoration("unknown", "•", "Contains changes"),
};

/** A folder takes the colour of its most pressing change. */
const TONE_RANK: Record<GitDecorationTone, number> = {
  conflict: 4,
  modified: 3,
  deleted: 2,
  added: 1,
  untracked: 0,
  unknown: -1,
};

/** Native `MAX_CHANGES`: Git status lists at most this many changes. */
export const MAX_LISTED_CHANGES = 300;

export function fileGitDecoration(change: FileChange): GitDecoration | null {
  if (isConflictedChange(change)) return FILE_DECORATIONS.conflict;
  const state = visibleChangeStatus(change);
  return state === "unchanged" || state === "unknown" || state === "conflicted"
    ? null
    : FILE_DECORATIONS[state];
}

/** Built once per refreshed root: refreshes keep unchanged roots (EP-12). */
const cache = new WeakMap<RootRepositoryStatus, RootGitDecorations>();

const EMPTY: RootGitDecorations = Object.freeze({
  files: new Map(),
  folders: new Map(),
});

/**
 * The marks for one root. Git lists at most 300 changes; past that the root
 * shows a "many changes" count, since Files beyond the cap are changed but
 * unmarked. Any other partial status marks the root as incomplete.
 */
export function rootGitDecorations(
  root: RootRepositoryStatus,
): RootGitDecorations {
  if (root.state !== "repository") return EMPTY;
  const cached = cache.get(root);
  if (cached) return cached;
  const files = new Map<string, GitDecoration>();
  const folderTones = new Map<string, GitDecorationTone>();
  for (const change of root.changes) {
    const mark = fileGitDecoration(change);
    if (!mark) continue;
    const path = change.resource.relativePath;
    files.set(path, mark);
    // Each folder above the File, then the root (""). A folder never ranks
    // below one inside it, so the walk stops at the first that already ranks
    // at least as high.
    for (
      let end = path.lastIndexOf("/");
      ;
      end = path.lastIndexOf("/", end - 1)
    ) {
      const folder = end > 0 ? path.slice(0, end) : "";
      const current = folderTones.get(folder);
      if (current !== undefined && TONE_RANK[current] >= TONE_RANK[mark.tone]) {
        break;
      }
      folderTones.set(folder, mark.tone);
      if (end <= 0) break;
    }
  }
  const folders = new Map<string, GitDecoration>();
  for (const [folder, tone] of folderTones) {
    folders.set(folder, FOLDER_DECORATIONS[tone]);
  }
  if (root.truncated) {
    // A full list proves more changes exist; anything else (output or line
    // counts cut short, an index-only projection) only that some may be
    // missing.
    const overflow = root.changes.length >= MAX_LISTED_CHANGES;
    folders.set(
      "",
      decoration(
        folderTones.get("") ?? "unknown",
        overflow ? `${MAX_LISTED_CHANGES}+` : "…",
        overflow
          ? `More than ${MAX_LISTED_CHANGES} changes. Only the first ${MAX_LISTED_CHANGES} are marked.`
          : "Git status is incomplete. Some changes may not be marked.",
      ),
    );
  }
  const result = { files, folders };
  cache.set(root, result);
  return result;
}

export function gitDecorationAt(
  decorations: RootGitDecorations | null,
  relativePath: string,
  directory: boolean,
): GitDecoration | undefined {
  return (directory ? decorations?.folders : decorations?.files)?.get(
    relativePath,
  );
}

/** The mark for an open File tab. */
export function resourceGitDecoration(
  repositories: ProjectRepositoryStatus,
  resource: ResourceRef,
): GitDecoration | undefined {
  const root = repositories.roots.find(
    (item) => item.rootId === resource.rootId,
  );
  return root
    ? rootGitDecorations(root).files.get(resource.relativePath)
    : undefined;
}
