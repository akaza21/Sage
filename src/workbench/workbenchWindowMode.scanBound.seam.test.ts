import { readFile } from "node:fs/promises";
import path from "node:path";
import { describe, expect, it } from "vitest";

import {
  AGENT_CENTER_CHAT_SCAN_LIMIT,
  type AgentCenterScanTab,
  agentCenterChatAction,
} from "./workbenchWindowMode";

const srcRoot = path.resolve(import.meta.dirname, "..");

describe("agentCenterChatAction scan bounds", () => {
  it("declares a finite positive scan bound", () => {
    expect(Number.isInteger(AGENT_CENTER_CHAT_SCAN_LIMIT)).toBe(true);
    expect(AGENT_CENTER_CHAT_SCAN_LIMIT).toBeGreaterThan(0);
    expect(AGENT_CENTER_CHAT_SCAN_LIMIT).toBeLessThanOrEqual(1024);
  });

  it("activates the first chat inside the bound without depending on later tabs", () => {
    const total = AGENT_CENTER_CHAT_SCAN_LIMIT * 3;
    const tabs: AgentCenterScanTab[] = Array.from(
      { length: total },
      (_, index): AgentCenterScanTab =>
        index === 5
          ? { id: "chat:first", kind: "chat" }
          : { id: `file:${index}`, kind: "file" },
    );
    tabs[AGENT_CENTER_CHAT_SCAN_LIMIT] = {
      id: "chat:outside",
      kind: "chat",
    };

    expect(agentCenterChatAction(tabs, "file:0")).toEqual({
      action: "activate",
      tabId: "chat:first",
    });
  });

  it("falls back to creating a chat when none exists inside the bound", () => {
    const total = AGENT_CENTER_CHAT_SCAN_LIMIT + 16;
    const tabs: AgentCenterScanTab[] = Array.from(
      { length: total },
      (_, index): AgentCenterScanTab =>
        index === AGENT_CENTER_CHAT_SCAN_LIMIT
          ? { id: "chat:outside", kind: "chat" }
          : { id: `file:${index}`, kind: "file" },
    );

    expect(agentCenterChatAction(tabs, "file:0")).toEqual({
      action: "create",
    });
  });

  it("treats an active chat inside the bound as already centered", () => {
    const tabs: AgentCenterScanTab[] = [
      { id: "file:0", kind: "file" },
      { id: "chat:active", kind: "chat" },
      { id: "file:2", kind: "file" },
    ];

    expect(agentCenterChatAction(tabs, "chat:active")).toEqual({
      action: "none",
    });
  });

  it("avoids unrestricted tab walks and untyped kinds in the module source", async () => {
    const source = await readFile(
      path.join(srcRoot, "workbench/workbenchWindowMode.ts"),
      "utf8",
    );
    expect(source).not.toMatch(/\.find\(/);
    expect(source).not.toMatch(/kind: string\b/);
  });
});
