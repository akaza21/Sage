import { describe, expect, it } from "vitest";

import { searchResultFileName } from "./searchResultCard";

describe("searchResultFileName", () => {
  it("shows the basename for Devin search cards", () => {
    expect(searchResultFileName("src/main.ts")).toBe("main.ts");
    expect(searchResultFileName("src\\main.ts")).toBe("main.ts");
    expect(searchResultFileName("README.md")).toBe("README.md");
    expect(searchResultFileName("")).toBe("");
  });
});
