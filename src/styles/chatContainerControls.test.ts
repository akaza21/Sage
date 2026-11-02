import { readFile } from "node:fs/promises";
import { describe, expect, it } from "vitest";

describe("Chat container control breakpoints", () => {
  it("truncates rather than removes the project name in the header menu", async () => {
    const header = await readFile("src/styles/agent-task-header.css", "utf8");
    expect(header).toMatch(
      /\.agent-header-more__project > span\s*\{[^}]*overflow:\s*hidden;[^}]*text-overflow:\s*ellipsis;[^}]*white-space:\s*nowrap;/s,
    );
    expect(header).not.toMatch(
      /\.agent-header-more__project > span\s*\{[^}]*display:\s*none;/s,
    );
  });
});
