import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";

import type {
  AcpItem,
  EntityRevision,
  RuntimeEpoch,
  SageAgentSessionId,
  SageChatId,
  SageItemId,
  SagePermissionRequestId,
  SageProjectId,
  SageTurnId,
} from "../../../domain/acp";
import { PermissionItem } from "./PermissionItem";
import { getPermissionStatus } from "./statusSemantics";

afterEach(cleanup);

type PermissionProjectionItem = Extract<AcpItem, { kind: "permissionRequest" }>;

function cancelledPermission(): PermissionProjectionItem {
  return {
    id: "permission-cancelled" as SageItemId,
    projectId: "project-1" as SageProjectId,
    chatId: "chat-1" as SageChatId,
    sessionId: "session-1" as SageAgentSessionId,
    turnId: "turn-1" as SageTurnId,
    runtimeEpoch: "1" as RuntimeEpoch,
    ordinal: 1,
    origin: "primary",
    activityId: null,
    lifecycle: "completed",
    revision: "1" as EntityRevision,
    createdAt: 1,
    updatedAt: 2,
    terminalAt: 2,
    kind: "permissionRequest",
    toolState: "cancelled",
    payload: {
      requestId: "request-1" as SagePermissionRequestId,
      state: "cancelled",
      label: "Write requested",
      summary: "The owning turn ended before the request was answered.",
    },
  };
}

describe("cancelled permission semantics", () => {
  it("maps cancellation to a neutral terminal status", () => {
    expect(getPermissionStatus(cancelledPermission())).toEqual({
      visual: "Cancelled",
      accessible: "Permission cancelled",
      terminal: true,
    });
  });

  it("renders an accessible terminal status without actions or waiting copy", () => {
    render(<PermissionItem item={cancelledPermission()} />);

    const status = screen.getByRole("status", { name: "Permission cancelled" });
    expect(status).toHaveTextContent("Cancelled");
    expect(status).toHaveAttribute("aria-live", "off");
    expect(screen.queryByText(/Waiting for approval/i)).toBeNull();
    expect(screen.queryByRole("note")).toBeNull();
    expect(screen.queryByRole("button")).toBeNull();
  });
});
