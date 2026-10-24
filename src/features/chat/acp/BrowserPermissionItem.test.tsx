import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import type { AcpItem } from "../../../domain/acp";
import { acpDesktop } from "../../../services/acpDesktop";
import { PermissionPrompt } from "./PermissionPrompt";
import { clearApprovalDecisions } from "../useApprovalSlot";

afterEach(() => {
  clearApprovalDecisions();
  cleanup();
  vi.restoreAllMocks();
});

const browserItem = {
  id: "permission-browser",
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
    requestId: "request-browser",
    state: "received",
    label: "Browser permission",
    summary: "Browser fill on https://example.com",
    source: "browser",
  },
} as Extract<AcpItem, { kind: "permissionRequest" }>;

describe("local browser permission card", () => {
  it("identifies the browser request and sends denial for its exact local identity", () => {
    const resolve = vi
      .spyOn(acpDesktop, "resolvePermissionDecision")
      .mockResolvedValue({
        projectId: "project-1",
        chatId: "chat-1",
        sessionId: "session-1",
        turnId: "turn-1",
        runtimeEpoch: "1",
        requestId: "request-browser",
        decision: "deny",
        state: "resolvedDeclined",
      });
    render(<PermissionPrompt items={[browserItem]} />);
    expect(
      screen.getByRole("group", { name: "Browser permission" }),
    ).toBeTruthy();
    expect(
      screen.getByText("Browser fill on https://example.com"),
    ).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: /Deny/ }));
    expect(resolve).toHaveBeenCalledWith({
      projectId: "project-1",
      chatId: "chat-1",
      sessionId: "session-1",
      turnId: "turn-1",
      runtimeEpoch: "1",
      requestId: "request-browser",
      decision: "deny",
    });
  });

  it("shows the tool and origin but never the fill value, with three decisions", () => {
    const resolve = vi
      .spyOn(acpDesktop, "resolvePermissionDecision")
      .mockResolvedValue({
        projectId: "project-1",
        chatId: "chat-1",
        sessionId: "session-1",
        turnId: "turn-1",
        runtimeEpoch: "1",
        requestId: "request-browser",
        decision: "allowForOriginSession",
        state: "resolvedAllowed",
      });
    render(<PermissionPrompt items={[browserItem]} />);
    expect(
      screen.getByText("Browser fill on https://example.com"),
    ).toBeTruthy();
    expect(screen.queryByText(/do-not-leak/)).toBeNull();
    expect(screen.getByRole("button", { name: /Allow once/ })).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "More allow options" }));
    fireEvent.click(
      screen.getByRole("menuitem", { name: "Allow for this tab and origin" }),
    );
    expect(resolve).toHaveBeenCalledWith(
      expect.objectContaining({
        requestId: "request-browser",
        decision: "allowForOriginSession",
      }),
    );
  });
});
