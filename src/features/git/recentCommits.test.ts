import { describe, expect, it } from "vitest";

import {
  admitRecentCommitSha,
  shortCommitSha,
  visibleRecentCommits,
} from "./recentCommits";

const FIRST = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

describe("recentCommits", () => {
  it("admits only a 40-character hexadecimal SHA", () => {
    expect(admitRecentCommitSha(FIRST)).toBe(FIRST);
    expect(admitRecentCommitSha(FIRST.toUpperCase())).toBe(FIRST);
    expect(admitRecentCommitSha("deadbeef")).toBeNull();
    expect(admitRecentCommitSha("g".repeat(40))).toBeNull();
    expect(admitRecentCommitSha("")).toBeNull();
  });

  it("shortens an admitted SHA to seven characters", () => {
    expect(shortCommitSha(FIRST)).toBe("aaaaaaa");
    expect(shortCommitSha("not-a-sha")).toBeNull();
  });

  it("keeps at most 64 admitted commits and drops invalid SHAs", () => {
    const overflow = Array.from({ length: 65 }, (_, index) => ({
      sha: `${index.toString(16).padStart(40, "0")}`,
      subject: `commit ${index}`,
    }));
    overflow[1] = { sha: "short", subject: "invalid" };
    const visible = visibleRecentCommits(overflow);
    expect(visible).toHaveLength(64);
    expect(visible[0]).toEqual({ sha: "0".repeat(40), subject: "commit 0" });
    expect(visible[1]).toEqual({
      sha: "2".padStart(40, "0"),
      subject: "commit 2",
    });
    expect(visible[63]?.subject).toBe("commit 64");
  });
});
