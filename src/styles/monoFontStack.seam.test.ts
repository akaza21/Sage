import { readFile } from "node:fs/promises";
import { resolve } from "node:path";

import { describe, expect, it } from "vitest";

describe("Monospace font stacks", () => {
  it("give macOS a real monospace face before the generic fallback", async () => {
    const global = await readFile(resolve("src/styles/global.css"), "utf8");
    const token = /--font-family-mono:([^;]+);/u.exec(global)?.[1] ?? "";
    const faces = token.split(",").map((face) => face.trim());
    // WebKit on macOS resolves a bare `monospace` to Courier.
    for (const face of ["ui-monospace", "Menlo", "Consolas"]) {
      expect(faces).toContain(face);
      expect(faces.indexOf(face)).toBeLessThan(faces.indexOf("monospace"));
    }
    expect(faces[0]).toBe('"Cascadia Code"');
  });

  it("leaves no stack that names SFMono-Regular without a macOS fallback", async () => {
    const { readdir } = await import("node:fs/promises");
    const files = (await readdir(resolve("src"), { recursive: true }))
      .filter((file) => file.endsWith(".css"))
      .map((file) => resolve("src", file));
    expect(files.length).toBeGreaterThan(10);
    for (const file of files) {
      const css = await readFile(file, "utf8");
      expect(css, file).not.toMatch(/font-family:[^;]*SFMono-Regular/u);
    }
    for (const file of [
      "src/styles/file-editor-surface.css",
      "src/styles/repository-sidebar-controls.css",
      "src/features/search/workspace-search.css",
    ]) {
      const css = await readFile(resolve(file), "utf8");
      expect(css).toContain("font-family: var(--font-family-mono);");
    }
  });
});
