import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import type { AcpItem } from "../../../domain/acp";
import { acpDesktop } from "../../../services/acpDesktop";
import { useChatAccessStore } from "../../../app/sessionAccessActions";
import { PermissionPrompt } from "./PermissionPrompt";
import { clearApprovalDecisions } from "../useApprovalSlot";

const access = vi.hoisted(() => ({
  get: vi.fn(async (): Promise<unknown> => "plan"),
  set: vi.fn(async () => null),
}));
vi.mock("../../../ports/chatAccessChanges", () => ({
  chatAccessChanges: () => access,
}));

type PermissionItem = Extract<AcpItem, { kind: "permissionRequest" }>;

const PLAN = "# Plan\n\n- Edit greet.js\n- Run it";

function planCard(id = "plan-card"): PermissionItem {
  return {
    id,
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
      requestId: "request-plan",
      state: "received",
      label: "Ready to code?",
      // The translator's summary for a mode switch: its label, then its text.
      summary: `Ready to code?\n\n${PLAN}`,
      offeredOptionKinds: ["allow_once", "allow_always", "reject_once"],
      category: "switchMode",
    },
  } as PermissionItem;
}

function resolved() {
  return vi
    .spyOn(acpDesktop, "resolvePermissionDecision")
    .mockResolvedValue({} as never);
}

beforeEach(() => {
  useChatAccessStore.setState({ modes: {}, error: null });
  access.get.mockResolvedValue("plan");
  access.set.mockClear();
});

afterEach(() => {
  cleanup();
  clearApprovalDecisions();
  vi.restoreAllMocks();
});

describe("PermissionPrompt plan approval", () => {
  it("asks the plan's question over the plan it approves", () => {
    render(<PermissionPrompt items={[planCard()]} />);
    expect(screen.getByText("Ready to code?")).toBeTruthy();
    expect(screen.queryByText("Allow this action?")).toBeNull();
    const plan = document.querySelector(".acp-permission-prompt__detail--plan");
    expect(plan?.textContent).toBe(PLAN);
  });

  it.each([
    ["Allow once", "ask"],
    ["Always allow", "acceptEdits"],
  ])("approving with %s moves a Plan chat to %s", async (label, mode) => {
    resolved();
    render(<PermissionPrompt items={[planCard()]} />);
    if (label === "Always allow") {
      fireEvent.click(screen.getByRole("button", { name: "More allow options" }));
      fireEvent.click(screen.getByRole("menuitem", { name: label }));
    } else {
      fireEvent.click(screen.getByRole("button", { name: /Allow once/ }));
    }
    await waitFor(() =>
      expect(access.set).toHaveBeenCalledWith("session-1", mode),
    );
    expect(useChatAccessStore.getState().modes["session-1"]).toBe(mode);
  });

  it("leaves a chat outside Plan, and a declined plan, where they were", async () => {
    resolved();
    access.get.mockResolvedValue("acceptEdits");
    render(<PermissionPrompt items={[planCard()]} />);
    fireEvent.click(screen.getByRole("button", { name: /Allow once/ }));
    await waitFor(() => expect(access.get).toHaveBeenCalled());
    cleanup();
    useChatAccessStore.setState({ modes: {}, error: null });
    access.get.mockResolvedValue("plan");
    render(<PermissionPrompt items={[planCard("plan-2")]} />);
    fireEvent.click(screen.getByRole("button", { name: /Deny/ }));
    await new Promise((resolve) => setTimeout(resolve, 0));
    expect(access.set).not.toHaveBeenCalled();
  });
});
