import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import type { AcpItem } from "../../../domain/acp";
import { PermissionItem } from "./PermissionItem";

afterEach(cleanup);

type PermissionProjectionItem = Extract<AcpItem, { kind: "permissionRequest" }>;

function permission(
  state: PermissionProjectionItem["payload"]["state"],
): PermissionProjectionItem {
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
      state,
      label: "Write requested",
      summary: "The harness requested a capability Sage does not allow.",
    },
  } as PermissionProjectionItem;
}

describe("PermissionItem", () => {
  it("fails closed with a deny action and no allow action", () => {
    const onDeny = vi.fn();
    render(<PermissionItem item={permission("received")} onDeny={onDeny} />);
    fireEvent.click(screen.getByRole("button", { name: "Deny permission" }));
    expect(onDeny).toHaveBeenCalledWith("request-1");
    expect(
      screen.queryByRole("button", { name: /allow/i }),
    ).not.toBeInTheDocument();
  });

  it("disables repeat denial after the durable decline starts", () => {
    render(<PermissionItem item={permission("declineCommitted")} />);
    expect(screen.getByRole("button")).toBeDisabled();
    expect(screen.getByRole("status")).toHaveTextContent(
      "Permission denial committed",
    );
  });

  it("does not offer an actionable denial before the bridge is wired", () => {
    render(<PermissionItem item={permission("received")} />);
    expect(
      screen.getByRole("button", { name: "Deny permission" }),
    ).toBeDisabled();
  });
});
