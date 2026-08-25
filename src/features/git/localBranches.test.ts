import { describe, expect, it } from "vitest";

import { currentLocalBranchName, isCurrentLocalBranch } from "./localBranches";

describe("localBranches", () => {
  it("treats a named heading as the current local branch", () => {
    expect(currentLocalBranchName("main")).toBe("main");
    expect(isCurrentLocalBranch("main", "main")).toBe(true);
    expect(isCurrentLocalBranch("topic", "main")).toBe(false);
  });

  it("does not treat detached HEAD labels as branch names", () => {
    expect(currentLocalBranchName("01234567 (detached)")).toBeNull();
    expect(currentLocalBranchName("HEAD (detached)")).toBeNull();
    expect(currentLocalBranchName(null)).toBeNull();
    expect(isCurrentLocalBranch("main", "01234567 (detached)")).toBe(false);
  });
});
