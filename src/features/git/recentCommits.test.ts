import { createElement } from "react";
import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
import { SourceCommitList } from "../../workbench/SourceCommitList";
afterEach(cleanup);
const FIRST = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
describe("recent commits through Source Control", () => {
  it("admits only a 40-character hexadecimal SHA", () => {
    render(
      createElement(SourceCommitList, {
        rootLabel: "Root",
        commits: [FIRST.toUpperCase(), "deadbeef", "g".repeat(40), ""].map(
          (sha) => ({ sha, subject: sha }),
        ),
      }),
    );
    expect(screen.getAllByRole("listitem")).toHaveLength(1);
    expect(screen.getByText("aaaaaaa")).toBeVisible();
  });
  it("shortens an admitted SHA to seven characters", () => {
    render(
      createElement(SourceCommitList, {
        rootLabel: "Root",
        commits: [
          { sha: FIRST, subject: "First" },
          { sha: "not-a-sha", subject: "Invalid" },
        ],
      }),
    );
    expect(screen.getByRole("listitem")).toHaveTextContent("aaaaaaaFirst");
    expect(screen.queryByText("Invalid")).toBeNull();
  });
  it("keeps at most 64 admitted commits and drops invalid SHAs", () => {
    const commits = Array.from({ length: 66 }, (_, index) => ({
      sha: index.toString(16).padStart(40, "0"),
      subject: `commit ${index}`,
    }));
    commits[1] = { sha: "short", subject: "invalid" };
    render(createElement(SourceCommitList, { rootLabel: "Root", commits }));
    const visible = screen.getAllByRole("listitem");
    expect(visible).toHaveLength(64);
    expect(visible[0]).toHaveTextContent("0000000commit 0");
    expect(visible[1]).toHaveTextContent("0000000commit 2");
    expect(visible[63]).toHaveTextContent("0000000commit 64");
    expect(screen.queryByText("commit 65")).toBeNull();
  });
});
