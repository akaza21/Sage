import { beforeEach, describe, expect, it } from "vitest";

import { browserDesktop, resetBrowserDesktopForTests } from "./browserDesktop";

describe("browser file creation fixture", () => {
  beforeEach(resetBrowserDesktopForTests);

  it("returns a typed authoritative listing and opens as an empty editor document", async () => {
    const result = await browserDesktop.createProjectFile({
      projectId: "browser-project",
      projectGeneration: 4,
      expectedTransitionEpoch: 0,
      parent: { rootId: "browser-root", relativePath: "src" },
      basename: "new.ts",
    });
    expect(result.resource).toEqual({
      rootId: "browser-root",
      relativePath: "src/new.ts",
    });
    expect(result.parentListing.entries).toContainEqual({
      name: "new.ts",
      resource: result.resource,
      kind: "file",
      byteLen: 0,
    });
    expect(
      browserDesktop.openEditorDocument(result.resource).buffer.content,
    ).toBe("");
  });

  it("rejects stale epochs, unavailable parents, collisions, and bounds", async () => {
    const base = {
      projectId: "browser-project",
      projectGeneration: 4,
      expectedTransitionEpoch: 0,
      parent: { rootId: "browser-root", relativePath: "src" },
      basename: "new.ts",
    };
    await browserDesktop.createProjectFile(base);
    await expect(browserDesktop.createProjectFile(base)).rejects.toThrow(
      "already exists",
    );
    await expect(
      browserDesktop.createProjectFile({
        ...base,
        basename: "later.ts",
        expectedTransitionEpoch: 1,
      }),
    ).rejects.toThrow("stale or unavailable");
    await expect(
      browserDesktop.createProjectFile({
        ...base,
        basename: "missing.ts",
        parent: { rootId: "browser-root", relativePath: "missing" },
      }),
    ).rejects.toThrow("parent is unavailable");
    await expect(
      browserDesktop.createProjectFile({ ...base, basename: "a".repeat(256) }),
    ).rejects.toThrow("UTF-8 bytes");
  });

  it("keeps the development fixture bounded", async () => {
    const base = {
      projectId: "browser-project",
      projectGeneration: 4,
      expectedTransitionEpoch: 0,
      parent: { rootId: "browser-root", relativePath: "src" },
      basename: "unused.ts",
    };
    for (let index = 0; index < 64; index += 1) {
      await browserDesktop.createProjectFile({
        ...base,
        basename: `new-${index}.ts`,
      });
    }
    await expect(
      browserDesktop.createProjectFile({ ...base, basename: "overflow.ts" }),
    ).rejects.toThrow("64-file limit");
  });
});
