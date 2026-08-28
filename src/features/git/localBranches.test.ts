import { describe, expect, it } from "vitest";

import {
  canCheckoutLocalBranch,
  canCreateLocalBranch,
  canDeleteLocalBranch,
  canRenameLocalBranch,
  currentLocalBranchName,
  isCurrentLocalBranch,
} from "./localBranches";

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

  it("allows checkout only for a different listed local branch", () => {
    expect(canCheckoutLocalBranch("topic", "main", ["main", "topic"])).toBe(
      true,
    );
    expect(canCheckoutLocalBranch("main", "main", ["main", "topic"])).toBe(
      false,
    );
    expect(canCheckoutLocalBranch("other", "main", ["main", "topic"])).toBe(
      false,
    );
    expect(
      canCheckoutLocalBranch("main", "01234567 (detached)", ["main"]),
    ).toBe(true);
  });

  it("admits an unlisted valid name and refuses listed or unsafe names", () => {
    expect(canCreateLocalBranch("topic", ["main"], false)).toBe(true);
    expect(canCreateLocalBranch("main", ["main"], false)).toBe(false);
    expect(canCreateLocalBranch("topic", ["main"], true)).toBe(false);
    expect(canCreateLocalBranch("-bad", ["main"], false)).toBe(false);
    expect(canCreateLocalBranch("has space", ["main"], false)).toBe(false);
    expect(canCreateLocalBranch("a..b", ["main"], false)).toBe(false);
    expect(canCreateLocalBranch("", ["main"], false)).toBe(false);
    expect(canCreateLocalBranch("m".repeat(256), ["main"], false)).toBe(false);
    expect(
      canCreateLocalBranch(
        "ok",
        Array.from({ length: 64 }, (_, index) => `b${index}`),
        false,
      ),
    ).toBe(false);
  });

  it("allows delete only for a listed non-current name on a complete list", () => {
    expect(
      canDeleteLocalBranch("topic", "main", ["main", "topic"], false),
    ).toBe(true);
    expect(canDeleteLocalBranch("main", "main", ["main", "topic"], false)).toBe(
      false,
    );
    expect(
      canDeleteLocalBranch("other", "main", ["main", "topic"], false),
    ).toBe(false);
    expect(canDeleteLocalBranch("topic", "main", ["main", "topic"], true)).toBe(
      false,
    );
    expect(
      canDeleteLocalBranch("main", "01234567 (detached)", ["main"], false),
    ).toBe(true);
  });

  it("admits rename from a listed name to a valid unlisted name", () => {
    expect(
      canRenameLocalBranch("topic", "feature", ["main", "topic"], false),
    ).toBe(true);
    expect(
      canRenameLocalBranch("main", "feature", ["main", "topic"], false),
    ).toBe(true);
    expect(
      canRenameLocalBranch("topic", "main", ["main", "topic"], false),
    ).toBe(false);
    expect(
      canRenameLocalBranch("other", "feature", ["main", "topic"], false),
    ).toBe(false);
    expect(
      canRenameLocalBranch("topic", "feature", ["main", "topic"], true),
    ).toBe(false);
    expect(
      canRenameLocalBranch("topic", "-bad", ["main", "topic"], false),
    ).toBe(false);
    expect(
      canRenameLocalBranch("topic", "topic", ["main", "topic"], false),
    ).toBe(false);
  });
});
