import { readFile } from "node:fs/promises";
import path from "node:path";
import { createElement } from "react";
import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import type { AcpItem } from "../../../domain/acp";
import { acpDesktop } from "../../../services/acpDesktop";
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
  it("keeps the product surface read-only until permission decisions are admitted", () => {
    const resolve = vi
      .spyOn(acpDesktop, "resolvePermissionDecision")
      .mockResolvedValue({
        projectId: "project-1",
        chatId: "chat-1",
        sessionId: "session-1",
        turnId: "turn-1",
        runtimeEpoch: "1",
        requestId: "request-1",
        decision: "deny",
        state: "declinePending",
      });

    render(createElement(PermissionItem, { item: waitingPermission() }));

    expect(
      screen.getByRole("note", {
        name: "",
      }),
    ).toHaveTextContent(
      "Permission actions are unavailable. Sage automatically denies permission requests in this build.",
    );
    expect(
      screen.queryByRole("button", { name: /ask|allow|deny/i }),
    ).toBeNull();
    expect(resolve).not.toHaveBeenCalled();
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
