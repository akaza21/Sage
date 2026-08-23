import { readFile } from "node:fs/promises";
import path from "node:path";
import { createElement } from "react";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import type { AcpItem, PermissionDecision } from "../../../domain/acp";
import { acpDesktop } from "../../../services/acpDesktop";
import { desktop } from "../../../services/desktop";
import { PermissionItem } from "./PermissionItem";

const acpDirectory = path.resolve(import.meta.dirname);

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
});

type PermissionProjectionItem = Extract<AcpItem, { kind: "permissionRequest" }>;

function waitingPermission(): PermissionProjectionItem {
  return {
    id: "permission-item",
    projectId: "project-1",
    chatId: "chat-1",
    sessionId: "session-1",
    turnId: "turn-1",
    runtimeEpoch: "1",
    ordinal: 1,
    origin: "primary",
    activityId: null,
    lifecycle: "streaming",
    revision: "1",
    createdAt: 1,
    updatedAt: 1,
    terminalAt: null,
    kind: "permissionRequest",
    toolState: "waitingForApproval",
    payload: {
      requestId: "request-1",
      state: "received",
      label: "Write requested",
      summary: "The harness requested a capability.",
    },
  } as PermissionProjectionItem;
}

describe("permission decision seam", () => {
  it("exposes exactly Ask, Allow once, and Deny through the desktop decision seam", async () => {
    const decisions = [
      "ask",
      "allowOnce",
      "deny",
    ] as const satisfies readonly PermissionDecision[];
    expect(decisions).toEqual(["ask", "allowOnce", "deny"]);

    const resolve = vi
      .spyOn(acpDesktop, "resolvePermissionDecision")
      .mockResolvedValue({
        projectId: "project-1",
        chatId: "chat-1",
        sessionId: "session-1",
        requestId: "request-1",
        decision: "deny",
        state: "declinePending",
      });
    const sendPrompt = vi.spyOn(desktop, "sendPrompt");

    render(createElement(PermissionItem, { item: waitingPermission() }));

    expect(screen.getByRole("button", { name: "Ask" })).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "Allow once" }),
    ).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Deny" }));

    expect(resolve).toHaveBeenCalledWith({
      projectId: "project-1",
      chatId: "chat-1",
      sessionId: "session-1",
      requestId: "request-1",
      decision: "deny",
    });
    expect(sendPrompt).not.toHaveBeenCalled();
  });

  it("keeps decision behavior harness-neutral", async () => {
    const sources = await Promise.all(
      [
        "PermissionItem.tsx",
        path.join(
          "..",
          "..",
          "..",
          "..",
          "src-tauri",
          "src",
          "acp",
          "runtime",
          "permissions.rs",
        ),
        path.join(
          "..",
          "..",
          "..",
          "..",
          "src-tauri",
          "src",
          "acp",
          "store",
          "requests.rs",
        ),
      ].map((file) => readFile(path.join(acpDirectory, file), "utf8")),
    );

    for (const source of sources) {
      expect(source).not.toMatch(/\b(?:codex|claude|pi|oh-my-pi)\b/i);
    }
  });
});
