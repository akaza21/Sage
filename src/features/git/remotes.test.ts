import { describe, expect, it } from "vitest";

import type { RootRepositoryStatus } from "../../domain/workbench";
import {
  admitRemoteName,
  canFetchRemote,
  canPushRemote,
  visibleRemotes,
} from "./remotes";

function root(
  overrides: Partial<RootRepositoryStatus> = {},
): RootRepositoryStatus {
  return {
    rootId: "root-a",
    state: "repository",
    branch: "main",
    remotes: ["origin", "upstream"],
    remotesTruncated: false,
    changes: [],
    truncated: false,
    elapsedMs: 1,
    message: null,
    ...overrides,
  };
}

describe("visibleRemotes", () => {
  it("admits a bounded unique remote list", () => {
    expect(admitRemoteName("origin")).toBe("origin");
    expect(admitRemoteName("-origin")).toBeNull();
    expect(visibleRemotes(["origin", " origin ", "upstream", ""])).toEqual([
      "origin",
      "upstream",
    ]);
    expect(
      visibleRemotes(Array.from({ length: 20 }, (_, index) => `r${index}`)),
    ).toHaveLength(16);
  });
});

describe("remote transport eligibility", () => {
  it("admits fetch and push only for a listed remote on an idle named branch", () => {
    expect(canFetchRemote(root(), "origin")).toBe(true);
    expect(canPushRemote(root(), "origin")).toBe(true);
    expect(canFetchRemote(root(), "missing")).toBe(false);
    expect(canPushRemote(root(), "missing")).toBe(false);
  });

  it("refuses fetch and push during history or conflict work", () => {
    expect(canFetchRemote(root({ mergeInProgress: true }), "origin")).toBe(
      false,
    );
    expect(canPushRemote(root({ rebaseInProgress: true }), "origin")).toBe(
      false,
    );
    expect(canFetchRemote(root({ cherryPickInProgress: true }), "origin")).toBe(
      false,
    );
    expect(canPushRemote(root({ revertInProgress: true }), "origin")).toBe(
      false,
    );
    expect(
      canFetchRemote(
        root({
          changes: [
            {
              resource: { rootId: "root-a", relativePath: "conflict.txt" },
              indexStatus: "conflicted",
              worktreeStatus: "conflicted",
              additions: null,
              deletions: null,
            },
          ],
        }),
        "origin",
      ),
    ).toBe(false);
  });

  it("refuses push when HEAD is detached and never treats a URL as a remote name", () => {
    expect(
      canPushRemote(root({ branch: "01234567 (detached)" }), "origin"),
    ).toBe(false);
    expect(canFetchRemote(root(), "https://example.invalid/repo.git")).toBe(
      false,
    );
    expect(admitRemoteName("https://example.invalid/repo.git")).toBeNull();
  });
});
