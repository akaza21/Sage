import { describe, expect, it } from "vitest";

import type { HarnessDefinition } from "../../../domain/harness";
import { admitsFocusedCodexRendererRecreation } from "./rendererRecreationCollectorAdmission";

const codex: HarnessDefinition = {
  id: "codex",
  name: "Codex",
  description: "Reviewed Codex",
  source: "registry",
  accountModelDiscovery: true,
  transport: "acp",
  installState: "available",
  bindingId: "codex-app-server",
};

describe("renderer recreation collector admission", () => {
  it("admits only the focused Agent Codex ACP page", () => {
    expect(
      admitsFocusedCodexRendererRecreation({
        windowMode: "agent",
        activeTabId: "chat:codex",
        focusedChatId: "chat:codex",
        harness: codex,
      }),
    ).toBe(true);
  });

  it.each([
    {
      windowMode: "editor" as const,
      activeTabId: "chat:codex",
      focusedChatId: "chat:codex",
      harness: codex,
    },
    {
      windowMode: "agent" as const,
      activeTabId: "chat:other",
      focusedChatId: "chat:codex",
      harness: codex,
    },
    {
      windowMode: "agent" as const,
      activeTabId: "chat:codex",
      focusedChatId: "chat:codex",
      harness: { ...codex, source: "development" as const },
    },
    {
      windowMode: "agent" as const,
      activeTabId: "chat:codex",
      focusedChatId: "chat:codex",
      harness: { ...codex, bindingId: "claude-code" },
    },
  ])("fails closed for an ineligible page", (candidate) => {
    expect(admitsFocusedCodexRendererRecreation(candidate)).toBe(false);
  });

  it("admits an eligible Codex page while discovery is rehydrating", () => {
    expect(
      admitsFocusedCodexRendererRecreation({
        windowMode: "agent",
        activeTabId: "chat:codex",
        focusedChatId: "chat:codex",
        harness: codex,
      }),
    ).toBe(true);
  });
});
