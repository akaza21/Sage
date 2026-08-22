import { readFile } from "node:fs/promises";
import path from "node:path";
import { describe, expect, it } from "vitest";

import { CURATED_READ_TOOL_NAME, createToolHost } from "./toolHost";

const acpDirectory = path.resolve(import.meta.dirname);

describe("curated read tool-host seam", () => {
  it("admits only the literal curated read tool", () => {
    const toolHost = createToolHost();

    expect(CURATED_READ_TOOL_NAME).toBe("sage.read_text");
    expect(toolHost.canInvoke("sage.read_text")).toBe(true);
    expect(toolHost.canInvoke("sage.write_text")).toBe(false);
    expect(toolHost.canInvoke("shell")).toBe(false);
    expect(toolHost.canInvoke("codex.exec")).toBe(false);
  });

  it("keeps tool admission harness-neutral", async () => {
    const source = await readFile(path.join(acpDirectory, "toolHost.ts"), "utf8");

    expect(source).not.toMatch(/codex|claude|pi/i);
  });

  it("keeps permission controls denial-only", async () => {
    const source = await readFile(
      path.join(acpDirectory, "PermissionItem.tsx"),
      "utf8",
    );

    expect(source).not.toMatch(
      /<button\b[^>]*(?:\b(?:allow|approve)\b)|<button\b[^>]*>(?:(?!<\/?button\b)[\s\S])*?\b(?:allow|approve)\b/i,
    );
  });
});
