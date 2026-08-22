import { readFile } from "node:fs/promises";
import path from "node:path";
import { describe, expect, it } from "vitest";

const srcRoot = path.resolve(import.meta.dirname, "..");

const SLICE_FILES = [
  "app/projectSlice.ts",
  "app/resourceSlice.ts",
  "app/diffLoad.ts",
  "app/fileCreateSlice.ts",
  "app/fileRenameSlice.ts",
  "app/resourceTrashSlice.ts",
] as const;

const EXPLORER_FILES = [
  "features/files/ExplorerSurface.tsx",
  "features/files/ExplorerActions.tsx",
] as const;

async function readSource(relative: string): Promise<string> {
  return readFile(path.join(srcRoot, relative), "utf8");
}

describe("S0.1 project/resource port seam", () => {
  it.each(SLICE_FILES)(
    "routes %l through getWorkbenchPorts instead of the aggregate desktop object",
    async (relative) => {
      const source = await readSource(relative);
      expect(source).not.toMatch(/from ["']\.\.\/services\/desktop["']/);
      expect(source).toMatch(/getWorkbenchPorts/);
    },
  );

  it("loads directories through the Project port", async () => {
    const source = await readSource("app/resourceSlice.ts");
    expect(source).toMatch(/getWorkbenchPorts\(\)\.project\.listDirectory/);
  });

  it("keeps getFileDiff on the carved-out Repository port only", async () => {
    const source = await readSource("app/diffLoad.ts");
    expect(source).not.toMatch(
      /(?:getWorkbenchPorts\(\)\.project|desktop)\.getFileDiff/,
    );
    expect(source).toMatch(/getWorkbenchPorts\(\)\.repository\.getFileDiff/);
  });

  it.each(EXPLORER_FILES)(
    "keeps %l selector-only without desktop or runtime port imports",
    async (relative) => {
      const source = await readSource(relative);
      expect(source).not.toMatch(/services\/desktop/);
      expect(source).not.toMatch(/getWorkbenchPorts/);
    },
  );

  it.each(EXPLORER_FILES)(
    "lets %l use the named open-Project selector instead of ports",
    async (relative) => {
      const source = await readSource(relative);
      if (/selectOpenProject/.test(source)) {
        expect(source).toMatch(
          /from ["']\.\.\/\.\.\/app\/workbenchSelectors["']/,
        );
      }
    },
  );
});
