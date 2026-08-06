import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";

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
  it("exposes only fail-closed permission semantics and details", () => {
    render(<PermissionItem item={permission("received")} />);
    expect(
      screen.getByRole("group", {
        name: "Permission request",
        description:
          "Sage declines permission requests in this read-only foundation",
      }),
    ).toBeInTheDocument();
    expect(screen.getByRole("status", { name: "Declining" })).toHaveAttribute(
      "aria-live",
      "assertive",
    );
    expect(screen.getByText("Details")).toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: /allow|approve|deny/i }),
    ).toBeNull();
  });

  it("announces a durable decline exactly once with the terminal name", () => {
    render(<PermissionItem item={permission("resolvedDeclined")} />);
    const status = screen.getByRole("status", {
      name: "Permission declined",
    });
    expect(status).toHaveTextContent("Declined");
    expect(status).toHaveAttribute("aria-live", "assertive");
    expect(status).toHaveAttribute("aria-atomic", "true");
  });
});
