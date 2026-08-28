import { readFile } from "node:fs/promises";
import path from "node:path";
import { describe, expect, it } from "vitest";

import {
  CURATED_APPLY_FILE_TOOL_NAME,
  CURATED_BROWSE_TOOL_NAME,
  CURATED_READ_TOOL_NAME,
  createToolHost,
} from "./toolHost";

const acpDirectory = path.resolve(import.meta.dirname);

describe("curated read tool-host seam", () => {
  it("admits only the literal curated read and typed apply tools", () => {
    const toolHost = createToolHost();

    expect(CURATED_READ_TOOL_NAME).toBe("sage.read_text");
    expect(CURATED_APPLY_FILE_TOOL_NAME).toBe("sage.apply_file");
    expect(CURATED_BROWSE_TOOL_NAME).toBe("sage.browse");
    expect(toolHost.canInvoke("sage.read_text")).toBe(true);
    expect(toolHost.canInvoke("sage.apply_file")).toBe(true);
    expect(toolHost.canInvoke("sage.browse")).toBe(true);
    expect(toolHost.canInvoke("sage.write_text")).toBe(false);
    expect(toolHost.canInvoke("shell")).toBe(false);
    expect(toolHost.canInvoke("codex.exec")).toBe(false);
  });

  it("keeps tool admission harness-neutral", async () => {
    const source = await readFile(
      path.join(acpDirectory, "toolHost.ts"),
      "utf8",
    );

    expect(source).not.toMatch(/codex|claude|pi/i);
  });

  it("delegates its optional apply tool to the typed intent helper", async () => {
    const source = await readFile(
      path.join(acpDirectory, "toolHost.ts"),
      "utf8",
    );

    expect(source).toContain("applyAgentFileIntent");
    expect(source).not.toMatch(/write_text|shell|codex|claude|oh-my-pi/i);
  });

  it("dispatches sage.browse through the curated host-tool invoke", async () => {
    const { browseSessionForChat, clearBrowseSessions } =
      await import("../../preview/harnessBrowse");
    clearBrowseSessions();
    const toolHost = createToolHost();
    expect(
      toolHost.invoke("sage.browse", {
        chatId: "chat:invoke",
        sessionId: "session-1",
        url: "http://127.0.0.1:5173/app",
      })?.href,
    ).toBe("http://127.0.0.1:5173/app");
    expect(browseSessionForChat("chat:invoke")?.href).toBe(
      "http://127.0.0.1:5173/app",
    );
    expect(
      toolHost.invoke("sage.read_text", {
        chatId: "chat:invoke",
        url: "http://127.0.0.1:5173/app",
      }),
    ).toBeNull();
    expect(
      toolHost.invoke("sage.browse", {
        chatId: "chat:invoke",
        sessionId: "session-1",
        url: "https://example.com/",
      }),
    ).toBeNull();
    expect(browseSessionForChat("chat:invoke")?.href).toBe(
      "http://127.0.0.1:5173/app",
    );
    clearBrowseSessions();
  });
});
