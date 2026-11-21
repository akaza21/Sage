import { describe, expect, expectTypeOf, it } from "vitest";

import type { TabKind } from "../domain/chat";
import {
  AGENT_CENTER_CHAT_SCAN_LIMIT,
  type AgentCenterScanTab,
  agentCenterNeedsDraft,
} from "./workbenchWindowMode";

describe("agentCenterNeedsDraft scan bounds", () => {
  it("scans no more than the 64-tab native layout maximum", () => {
    expect(AGENT_CENTER_CHAT_SCAN_LIMIT).toBe(64);
    const tabs: AgentCenterScanTab[] = Array.from(
      { length: 65 },
      (_, index) => ({
        id: `file:${index}`,
        kind: index === 64 ? "chat" : "file",
      }),
    );
    expect(agentCenterNeedsDraft(tabs, null)).toBe(true);
    tabs[63] = { id: "chat:63", kind: "chat" };
    expect(agentCenterNeedsDraft(tabs, null)).toBe(false);
  });

  it("declares a finite positive scan bound", () => {
    expect(Number.isInteger(AGENT_CENTER_CHAT_SCAN_LIMIT)).toBe(true);
    expect(AGENT_CENTER_CHAT_SCAN_LIMIT).toBeGreaterThan(0);
    expect(AGENT_CENTER_CHAT_SCAN_LIMIT).toBeLessThanOrEqual(1024);
  });

  it("leaves an in-bound chat in place without depending on later tabs", () => {
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

    expect(agentCenterNeedsDraft(tabs, "file:0")).toBe(false);
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

    expect(agentCenterNeedsDraft(tabs, "file:0")).toBe(true);
  });

  it("treats an active chat inside the bound as already centered", () => {
    const tabs: AgentCenterScanTab[] = [
      { id: "file:0", kind: "file" },
      { id: "chat:active", kind: "chat" },
      { id: "file:2", kind: "file" },
    ];

    expect(agentCenterNeedsDraft(tabs, "chat:active")).toBe(false);
  });

  it("never reads a tab beyond the native scan bound", () => {
    const tabs: AgentCenterScanTab[] = Array.from(
      { length: AGENT_CENTER_CHAT_SCAN_LIMIT + 1 },
      (_, index) => ({
        id: `file:${index}`,
        kind: index === AGENT_CENTER_CHAT_SCAN_LIMIT ? "chat" : "file",
      }),
    );
    const guarded = new Proxy(tabs, {
      get(target, property, receiver) {
        if (
          typeof property === "string" &&
          /^\d+$/.test(property) &&
          Number(property) >= AGENT_CENTER_CHAT_SCAN_LIMIT
        ) {
          throw new Error(`Out-of-bound tab read: ${property}`);
        }
        return Reflect.get(target, property, receiver);
      },
    });

    expect(agentCenterNeedsDraft(guarded, "file:0")).toBe(true);
  });

  it("keeps tab kinds limited to the domain tab-kind union", () => {
    expectTypeOf<AgentCenterScanTab["kind"]>().toEqualTypeOf<TabKind>();
  });
});

it("opens a draft instead of implicitly choosing a hidden empty chat, while preserving an explicitly open one", () => {
  const tabs = [
    { id: "legacy", kind: "chat" as const, chatListed: false },
    { id: "file", kind: "file" as const },
  ];
  expect(agentCenterNeedsDraft(tabs, null)).toBe(true);
  expect(agentCenterNeedsDraft(tabs, "file")).toBe(true);
  expect(agentCenterNeedsDraft(tabs, "legacy")).toBe(false);
  expect(agentCenterNeedsDraft(tabs, "file", "legacy")).toBe(false);
});
