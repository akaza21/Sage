import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import type { AcpItem } from "../../../domain/acp";
import { acpDesktop } from "../../../services/acpDesktop";
import { PermissionPrompt } from "./PermissionPrompt";
import { clearApprovalDecisions } from "../useApprovalSlot";
import { ackPopup } from "../../../app/browserPopupNative";
import {
  clearAgentChatOwner,
  pinAgentChatOwner,
} from "../../../app/agentChatOwner";
import * as browserPort from "../../../ports/nativeBrowserPort";

afterEach(() => {
  clearAgentChatOwner();
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
  it("keeps a pending browser card visible and resolvable after popup rollback", async () => {
    vi.spyOn(browserPort, "browserPopupAck").mockRejectedValue(
      new Error("popup acknowledgement unavailable"),
    );
    vi.spyOn(browserPort, "browserPopupAbandon").mockResolvedValue(undefined);
    const resolve = vi
      .spyOn(acpDesktop, "resolvePermissionDecision")
      .mockResolvedValue({
        source: "browser",
        projectId: "project-1",
        chatId: "chat-1",
        sessionId: "session-1",
        turnId: "turn-1",
        runtimeEpoch: "1",
        requestId: "request-browser",
        decision: "allowOnce",
        state: "resolvedAllowed",
      });
    const activateChat = vi.fn();
    const state = {
      tabs: [
        { id: "chat-1", kind: "chat", title: "Chat", position: 0 },
        { id: "browser-4", kind: "browser", title: "Popup", position: 1 },
      ],
      activeTabId: "browser-4",
      activateChat,
    };
    pinAgentChatOwner("chat-1");
    render(<PermissionPrompt items={[browserItem]} />);
    await expect(
      ackPopup(
        ((update: object) => Object.assign(state, update)) as never,
        (() => state) as never,
        21,
        {
          tabId: "browser-4",
          openerTabId: "browser-1",
          url: "https://example.com/",
        },
        true,
      ),
    ).rejects.toThrow("popup acknowledgement unavailable");
    expect(activateChat).not.toHaveBeenCalled();
    expect(
      screen.getByRole("group", { name: "Browser permission" }),
    ).toBeVisible();
    fireEvent.click(screen.getByRole("button", { name: /Allow once/ }));
    expect(resolve).toHaveBeenCalledWith(
      expect.objectContaining({
        requestId: "request-browser",
        decision: "allowOnce",
        chatId: "chat-1",
      }),
    );
  });

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

  it("asks for developer mode with only Allow developer mode and Deny, and Enter never approves", () => {
    const resolve = vi
      .spyOn(acpDesktop, "resolvePermissionDecision")
      .mockResolvedValue({
        source: "browser",
        projectId: "project-1",
        chatId: "chat-1",
        sessionId: "session-1",
        turnId: "turn-1",
        runtimeEpoch: "1",
        requestId: "request-developer",
        decision: "allowDeveloperMode",
        state: "resolvedAllowed",
      });
    const developerItem = {
      ...browserItem,
      id: "permission-developer",
      payload: {
        ...browserItem.payload,
        requestId: "request-developer",
        label: "Browser developer mode",
        summary: "Browser developer mode on https://example.com",
        source: "browserDeveloper",
      },
    } as Extract<AcpItem, { kind: "permissionRequest" }>;
    render(<PermissionPrompt items={[developerItem]} />);
    expect(screen.getByText("Allow developer mode on this site?")).toBeTruthy();
    expect(screen.getByText("Elevated")).toBeTruthy();
    expect(
      screen.getByText(/read this site's cookies and session data/),
    ).toBeTruthy();
    expect(screen.queryByRole("button", { name: /Allow once/ })).toBeNull();
    expect(
      screen.queryByRole("button", { name: "More allow options" }),
    ).toBeNull();
    const allow = screen.getByRole("button", { name: "Allow developer mode" });
    expect(document.activeElement).not.toBe(allow);
    expect(allow.querySelector("kbd")).toBeNull();
    fireEvent.click(allow);
    expect(resolve).toHaveBeenCalledWith(
      expect.objectContaining({
        requestId: "request-developer",
        decision: "allowDeveloperMode",
      }),
    );
  });
});
