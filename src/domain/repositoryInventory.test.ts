import { describe, expect, it } from "vitest";

import {
  isRootRepository,
  repositoryKey,
  sameRepository,
} from "./repositoryInventory";

describe("repository inventory identity", () => {
  it("keeps the empty relative path as the root repository identity", () => {
    const root = { rootId: "root-a", relativePath: "" };
    const nested = { rootId: "root-a", relativePath: "packages/tool" };

    expect(isRootRepository(root)).toBe(true);
    expect(repositoryKey(root)).toBe("root-a\u0000");
    expect(sameRepository(root, nested)).toBe(false);
  });
});
