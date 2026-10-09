import { readFile } from "node:fs/promises";
import path from "node:path";
import { describe, expect, it } from "vitest";
import type { HarnessBrowseOwner } from "../../preview/harnessBrowse";

import { CURATED_BROWSE_TOOL_NAME, invokeBrowseTool } from "./toolHost";

const acpDirectory = path.resolve(import.meta.dirname);

describe("curated tool-host seam", () => {
  it("invokes only the curated browse capability with native owner and URL admission", async () => {
    const { clearBrowseSessions, setHarnessBrowseOwner } =
      await import("../../preview/harnessBrowse");
    clearBrowseSessions();
    const owner = {
      projectId: "project:narrow",
      projectGeneration: "1",
      transitionEpoch: 1,
      chatId: "chat:narrow",
      sessionId: "session-1",
      runtimeEpoch: "1",
    } as HarnessBrowseOwner;
    setHarnessBrowseOwner(owner, [owner.chatId]);
    const intent = { owner, url: "http://127.0.0.1:5173/app" };
    expect(invokeBrowseTool("sage.apply_file", intent)).toBeNull();
    expect(invokeBrowseTool(CURATED_BROWSE_TOOL_NAME, intent)?.href).toBe(
      intent.url,
    );
    expect(
      invokeBrowseTool(CURATED_BROWSE_TOOL_NAME, {
        ...intent,
        url: "https://example.com/",
      }),
    ).toBeNull();
    expect(
      invokeBrowseTool(CURATED_BROWSE_TOOL_NAME, {
        ...intent,
        owner: { ...owner, runtimeEpoch: "2" as never },
      }),
    ).toBeNull();
    clearBrowseSessions();
  });
  it("keeps tool admission harness-neutral", async () => {
    const source = await readFile(
      path.join(acpDirectory, "toolHost.ts"),
      "utf8",
    );

    expect(source).not.toMatch(/codex|claude|pi/i);
  });
});
