import { describe, expect, it } from "vitest";

import type {
  FileChange,
  GitFileState,
  ProjectRepositoryStatus,
  RootRepositoryStatus,
} from "../../domain/repository";
import {
  gitDecorationAt,
  resourceGitDecoration,
  rootGitDecorations,
} from "./gitDecorations";

function change(
  relativePath: string,
  worktreeStatus: GitFileState,
  indexStatus: GitFileState = "unchanged",
): FileChange {
  return {
    resource: { rootId: "root-a", relativePath },
    indexStatus,
    worktreeStatus,
    additions: null,
    deletions: null,
  };
}

function root(
  changes: FileChange[],
  extra: Partial<RootRepositoryStatus> = {},
): RootRepositoryStatus {
  return {
    rootId: "root-a",
    state: "repository",
    branch: "main",
    changes,
    truncated: false,
    elapsedMs: 4,
    message: null,
    ...extra,
  };
}

describe("rootGitDecorations", () => {
  it("marks each changed File with VS Code's letter and tone", () => {
    const marks = rootGitDecorations(
      root([
        change("src/app.ts", "modified"),
        change("src/new.ts", "unchanged", "added"),
        change("notes.md", "untracked"),
        change("old.txt", "deleted"),
        change("moved.ts", "unchanged", "renamed"),
        change("merge.ts", "conflicted", "conflicted"),
        change("same.ts", "unchanged"),
      ]),
    );
    const badge = (path: string) => {
      const mark = marks.files.get(path);
      return mark && [mark.badge, mark.tone, mark.label];
    };
    expect(badge("src/app.ts")).toEqual(["M", "modified", "Modified"]);
    expect(badge("src/new.ts")).toEqual(["A", "added", "Added"]);
    expect(badge("notes.md")).toEqual(["U", "untracked", "Untracked"]);
    expect(badge("old.txt")).toEqual(["D", "deleted", "Deleted"]);
    expect(badge("moved.ts")).toEqual(["R", "added", "Renamed"]);
    expect(badge("merge.ts")).toEqual(["!", "conflict", "Conflict"]);
    expect(marks.files.has("same.ts")).toBe(false);
  });

  it("shows the working tree over the index, as Source Control does", () => {
    const marks = rootGitDecorations(
      root([change("a.ts", "modified", "added")]),
    );
    expect(marks.files.get("a.ts")?.badge).toBe("M");
  });

  it("marks staged Files from the post-commit index-only status", () => {
    const marks = rootGitDecorations(
      root([change("a.ts", "unknown", "added")], { truncated: true }),
    );
    expect(marks.files.get("a.ts")?.badge).toBe("A");
  });

  it("tints every folder above a change with its most pressing tone", () => {
    const marks = rootGitDecorations(
      root([
        change("src/a/b/new.ts", "untracked"),
        change("src/a/edit.ts", "modified"),
        change("src/z/gone.ts", "deleted"),
        change("src/z/q/clash.ts", "conflicted"),
      ]),
    );
    const tone = (path: string) => marks.folders.get(path)?.tone;
    expect(tone("src/a/b")).toBe("untracked");
    expect(tone("src/a")).toBe("modified");
    expect(tone("src/z/q")).toBe("conflict");
    expect(tone("src/z")).toBe("conflict");
    expect(tone("src")).toBe("conflict");
    expect(tone("")).toBe("conflict");
    expect(marks.folders.get("src")).toMatchObject({
      badge: "•",
      label: "Contains conflicts",
    });
    expect(marks.folders.has("docs")).toBe(false);
    // Files are not folders.
    expect(gitDecorationAt(marks, "src/a/edit.ts", true)).toBeUndefined();
    expect(gitDecorationAt(marks, "src/a/edit.ts", false)?.badge).toBe("M");
  });

  it("raises a folder after a weaker change marked it first", () => {
    const marks = rootGitDecorations(
      root([change("a/b/one.ts", "untracked"), change("a/two.ts", "deleted")]),
    );
    expect(marks.folders.get("a/b")?.tone).toBe("untracked");
    expect(marks.folders.get("a")?.tone).toBe("deleted");
    expect(marks.folders.get("")?.tone).toBe("deleted");
  });

  it("marks the first 300 changes and shows many changes on the root past the cap", () => {
    const changes = Array.from({ length: 300 }, (_, index) =>
      change(`gen/file-${index}.ts`, "untracked"),
    );
    const marks = rootGitDecorations(
      root(changes, { truncated: true, changesOverflowed: true }),
    );
    expect(marks.files.size).toBe(300);
    expect(marks.folders.get("gen")?.badge).toBe("•");
    expect(marks.folders.get("")).toEqual({
      tone: "untracked",
      badge: "300+",
      label: "More than 300 changes. Only the first 300 are marked.",
    });
  });

  it("marks a partial status as incomplete, not as a change count", () => {
    // After a commit the index-only projection is clean but partial.
    const clean = rootGitDecorations(root([], { truncated: true }));
    expect(clean.files.size).toBe(0);
    expect(clean.folders.get("")).toEqual({
      tone: "unknown",
      badge: "…",
      label: "Git status is incomplete. Some changes may not be marked.",
    });
    // Line counts cut short with a few changes listed.
    const partial = rootGitDecorations(
      root([change("src/a.ts", "modified")], { truncated: true }),
    );
    expect(partial.folders.get("")).toMatchObject({
      tone: "modified",
      badge: "…",
    });
    expect(partial.folders.get("src")?.badge).toBe("•");
    expect(partial.files.get("src/a.ts")?.badge).toBe("M");
    // Exactly 300 staged changes in the index-only projection: no count.
    const full = rootGitDecorations(
      root(
        Array.from({ length: 300 }, (_, index) =>
          change(`staged-${index}.ts`, "unchanged", "added"),
        ),
        { truncated: true },
      ),
    );
    expect(full.folders.get("")).toMatchObject({ badge: "…" });
  });

  it("builds once per root object and reuses decoration objects", () => {
    const first = root([change("a.ts", "modified")]);
    expect(rootGitDecorations(first)).toBe(rootGitDecorations(first));
    const second = root([change("a.ts", "modified")]);
    expect(rootGitDecorations(second)).not.toBe(rootGitDecorations(first));
    // A row whose mark did not change gets the same prop.
    expect(rootGitDecorations(second).files.get("a.ts")).toBe(
      rootGitDecorations(first).files.get("a.ts"),
    );
  });

  it("marks nothing outside a repository", () => {
    const marks = rootGitDecorations(
      root([change("a.ts", "modified")], { state: "notRepository" }),
    );
    expect(marks.files.size).toBe(0);
    expect(marks.folders.size).toBe(0);
  });
});

describe("resourceGitDecoration", () => {
  it("finds a tab's File in its own root", () => {
    const repositories: ProjectRepositoryStatus = {
      roots: [
        root([change("a.ts", "modified")]),
        { ...root([change("a.ts", "deleted")]), rootId: "root-b" },
      ],
      truncated: false,
      elapsedMs: 1,
    };
    expect(
      resourceGitDecoration(repositories, {
        rootId: "root-b",
        relativePath: "a.ts",
      })?.badge,
    ).toBe("D");
    expect(
      resourceGitDecoration(repositories, {
        rootId: "root-c",
        relativePath: "a.ts",
      }),
    ).toBeUndefined();
  });
});
