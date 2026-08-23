import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";

import type { AcpItem } from "../../../domain/acp";
import { PermissionItem } from "./PermissionItem";

afterEach(cleanup);

type PermissionProjectionItem = Extract<AcpItem, { kind: "permissionRequest" }>;

function permission(
  state: PermissionProjectionItem["payload"]["state"],
  failedClosed = false,
): PermissionProjectionItem {
  const declined = state === "resolvedDeclined" && !failedClosed;
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
    lifecycle: failedClosed ? "failed" : declined ? "completed" : "streaming",
    revision: "1",
    createdAt: 1,
    updatedAt: 1,
    terminalAt: failedClosed || declined ? 2 : null,
    kind: "permissionRequest",
    toolState: failedClosed
      ? "failed"
      : declined
        ? "declined"
        : "waitingForApproval",
    payload: {
      requestId: "request-1",
      state,
      label: "Write requested",
      summary: "The harness requested a capability Sage does not allow.",
    },
  } as PermissionProjectionItem;
}

describe("PermissionItem", () => {
  it("offers Ask, Allow once, and Deny while approval is waiting", () => {
    render(<PermissionItem item={permission("received")} />);
    expect(
      screen.getByRole("group", {
        name: "Permission request",
        description: "Choose how Sage should handle this permission request",
      }),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("status", { name: "Waiting for approval" }),
    ).toHaveAttribute("aria-live", "off");
    expect(screen.getByText("Details")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Ask" })).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "Allow once" }),
    ).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Deny" })).toBeInTheDocument();
  });

  it("retains a durable decline with the terminal name but live-off", () => {
    render(<PermissionItem item={permission("resolvedDeclined")} />);
    const status = screen.getByRole("status", {
      name: "Permission declined",
    });
    expect(status).toHaveTextContent("Declined");
    expect(status).toHaveAttribute("aria-live", "off");
    expect(status).toHaveAttribute("aria-atomic", "true");
  });

  it("retains an unresolved permission terminalized by teardown as failed closed", () => {
    render(<PermissionItem item={permission("resolvedDeclined", true)} />);
    const status = screen.getByRole("status", {
      name: "Permission failed closed",
    });
    expect(status).toHaveTextContent("Failed closed");
    expect(status).toHaveAttribute("aria-live", "off");
    expect(status).toHaveAttribute("aria-atomic", "true");
    expect(screen.queryByRole("button", { name: /allow|approve/i })).toBeNull();
  });
});
