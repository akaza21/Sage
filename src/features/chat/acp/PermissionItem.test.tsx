import {
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import type { AcpItem } from "../../../domain/acp";
import { acpDesktop } from "../../../services/acpDesktop";
import { PermissionItem } from "./PermissionItem";

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
});

type PermissionProjectionItem = Extract<AcpItem, { kind: "permissionRequest" }>;

function permission(
  state: PermissionProjectionItem["payload"]["state"],
  failedClosed = false,
  offeredOptionKinds: readonly string[] | null = ["allow_once", "reject_once"],
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
      offeredOptionKinds: offeredOptionKinds ?? undefined,
    },
  } as PermissionProjectionItem;
}

describe("PermissionItem", () => {
  it("does not offer approval when the harness offered only rejection", () => {
    const resolve = vi.spyOn(acpDesktop, "resolvePermissionDecision");
    const card = permission("received", false, ["reject_once"]);
    render(<PermissionItem item={card} />);
    expect(screen.queryByRole("button", { name: "Approve" })).toBeNull();
    expect(screen.getByRole("button", { name: "Deny" })).toBeEnabled();
    fireEvent.keyDown(
      screen.getByRole("group", { name: "Permission request" }),
      { key: "Enter" },
    );
    expect(resolve).not.toHaveBeenCalled();
  });

  it("shows only denial for a legacy item without offered options", () => {
    render(<PermissionItem item={permission("received", false, null)} />);
    expect(screen.queryByRole("button", { name: "Approve" })).toBeNull();
    expect(screen.getByRole("button", { name: "Deny" })).toBeEnabled();
  });
  it("shows only the harness-offered always approval", () => {
    render(
      <PermissionItem
        item={permission("received", false, ["allow_always", "reject_always"])}
      />,
    );
    expect(screen.queryByRole("button", { name: "Approve" })).toBeNull();
    expect(screen.getByRole("button", { name: "Always allow" })).toBeEnabled();
    expect(screen.getByRole("button", { name: "Deny" })).toBeEnabled();
  });
  it("preserves the harness's approval option order", () => {
    render(
      <PermissionItem
        item={permission("received", false, [
          "allow_always",
          "allow_once",
          "reject_once",
        ])}
      />,
    );
    expect(
      screen.getAllByRole("button").map((button) => button.textContent),
    ).toEqual(["Always allow", "Approve", "Deny"]);
  });
  it("sends the exact always decision for a harness-offered option", () => {
    const resolve = vi
      .spyOn(acpDesktop, "resolvePermissionDecision")
      .mockResolvedValue({
        projectId: "project-1",
        chatId: "chat-1",
        sessionId: "session-1",
        turnId: "turn-1",
        runtimeEpoch: "1",
        requestId: "request-1",
        decision: "allowAlways",
        state: "allowPending",
      });
    render(
      <PermissionItem item={permission("received", false, ["allow_always"])} />,
    );
    fireEvent.click(screen.getByRole("button", { name: "Always allow" }));
    expect(resolve).toHaveBeenCalledWith(
      expect.objectContaining({ decision: "allowAlways" }),
    );
  });
  it("shows what is being asked without expanding a disclosure", () => {
    render(<PermissionItem item={permission("received")} />);
    const group = screen.getByRole("group", { name: "Permission request" });
    expect(group.querySelector("details")).toBeNull();
    expect(
      screen.getByText(
        "The harness requested a capability Sage does not allow.",
      ),
    ).toBeVisible();
    expect(group).toHaveAttribute("data-activity-state", "waitingForApproval");
    expect(group).toHaveAttribute("data-acp-item-id", "permission-item");
  });

  it("offers approve and deny on a waiting permission", () => {
    render(<PermissionItem item={permission("received")} />);
    expect(
      screen.getByRole("group", { name: "Permission request" }),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("status", { name: "Waiting for approval" }),
    ).toHaveAttribute("aria-live", "off");
    expect(screen.getByRole("button", { name: "Approve" })).toBeEnabled();
    expect(screen.getByRole("button", { name: "Deny" })).toBeEnabled();
    expect(
      screen.queryByText(
        "Permission actions are unavailable. Sage automatically denies permission requests in this build.",
      ),
    ).toBeNull();
  });

  it("resolves an approval through the six-field request identity", async () => {
    const resolve = vi
      .spyOn(acpDesktop, "resolvePermissionDecision")
      .mockResolvedValue({
        projectId: "project-1",
        chatId: "chat-1",
        sessionId: "session-1",
        turnId: "turn-1",
        runtimeEpoch: "1",
        requestId: "request-1",
        decision: "allowOnce",
        state: "allowPending",
      });
    render(<PermissionItem item={permission("received")} />);
    fireEvent.click(screen.getByRole("button", { name: "Approve" }));
    expect(resolve).toHaveBeenCalledWith({
      projectId: "project-1",
      chatId: "chat-1",
      sessionId: "session-1",
      turnId: "turn-1",
      runtimeEpoch: "1",
      requestId: "request-1",
      decision: "allowOnce",
    });
    await waitFor(() =>
      expect(screen.getByRole("button", { name: "Approve" })).toBeDisabled(),
    );
  });

  it("shows a visible error when an offered action is refused by the binding", async () => {
    vi.spyOn(acpDesktop, "resolvePermissionDecision").mockRejectedValue(
      new Error("unsupported approval"),
    );
    render(<PermissionItem item={permission("received")} />);
    fireEvent.click(screen.getByRole("button", { name: "Approve" }));
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "This permission action could not be completed",
    );
    expect(screen.getByRole("button", { name: "Deny" })).toBeEnabled();
  });

  it("resolves a denial with the deny decision", () => {
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
    render(<PermissionItem item={permission("received")} />);
    fireEvent.click(screen.getByRole("button", { name: "Deny" }));
    expect(resolve).toHaveBeenCalledWith(
      expect.objectContaining({ decision: "deny" }),
    );
  });

  it("approves on Enter and denies on Escape while the card is focused", () => {
    const resolve = vi
      .spyOn(acpDesktop, "resolvePermissionDecision")
      .mockResolvedValue({
        projectId: "project-1",
        chatId: "chat-1",
        sessionId: "session-1",
        turnId: "turn-1",
        runtimeEpoch: "1",
        requestId: "request-1",
        decision: "allowOnce",
        state: "allowPending",
      });
    const { unmount } = render(
      <PermissionItem item={permission("received")} />,
    );
    const card = screen.getByRole("group", { name: "Permission request" });
    expect(card).toHaveAttribute("tabindex", "0");
    fireEvent.keyDown(card, { key: "Enter" });
    expect(resolve).toHaveBeenLastCalledWith(
      expect.objectContaining({ decision: "allowOnce" }),
    );
    unmount();

    render(<PermissionItem item={permission("received")} />);
    fireEvent.keyDown(
      screen.getByRole("group", { name: "Permission request" }),
      { key: "Escape" },
    );
    expect(resolve).toHaveBeenLastCalledWith(
      expect.objectContaining({ decision: "deny" }),
    );
  });

  it("ignores the keyboard path once the permission is no longer waiting", () => {
    const resolve = vi.spyOn(acpDesktop, "resolvePermissionDecision");
    render(<PermissionItem item={permission("resolvedDeclined")} />);
    const card = screen.getByRole("group", { name: "Permission request" });
    expect(card).not.toHaveAttribute("tabindex");
    fireEvent.keyDown(card, { key: "Enter" });
    fireEvent.keyDown(card, { key: "Escape" });
    expect(resolve).not.toHaveBeenCalled();
  });

  it("shows the projected allowed and declined outcomes", () => {
    render(<PermissionItem item={permission("resolvedAllowed")} />);
    expect(
      screen.getByRole("status", { name: "Permission allowed" }),
    ).toHaveTextContent("Allowed");
    expect(screen.queryByRole("button", { name: "Approve" })).toBeNull();
  });

  it("does not mislabel an always approval as allowed once after settlement", () => {
    render(
      <PermissionItem
        item={permission("resolvedAllowed", false, ["allow_always"])}
      />,
    );
    expect(
      screen.getByRole("status", { name: "Permission allowed" }),
    ).toHaveTextContent("Allowed");
    expect(screen.queryByText("Allowed once")).toBeNull();
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

  it("keeps a settled decision on one muted disclosure row", () => {
    const { container } = render(
      <PermissionItem item={permission("resolvedDeclined")} />,
    );
    const row = container.querySelector(".acp-permission--settled");
    expect(row).not.toBeNull();
    expect(row?.querySelector("header")).toBeNull();
    expect(row?.querySelector("summary")).toHaveTextContent(
      "Write requested · Declined",
    );
    expect(row?.querySelector(".acp-permission__actions")).toBeNull();
  });

  it("carries the item identity and activity state the tool card exposes", () => {
    const { container, rerender } = render(
      <PermissionItem item={permission("received")} />,
    );
    const waiting = container.querySelector(".acp-permission");
    expect(waiting).toHaveAttribute("data-acp-item-id", "permission-item");
    expect(waiting).toHaveAttribute(
      "data-activity-state",
      "waitingForApproval",
    );

    rerender(<PermissionItem item={permission("resolvedDeclined")} />);
    const declined = container.querySelector(".acp-permission");
    expect(declined).toHaveAttribute("data-acp-item-id", "permission-item");
    expect(declined).toHaveAttribute("data-activity-state", "declined");

    rerender(<PermissionItem item={permission("resolvedDeclined", true)} />);
    expect(container.querySelector(".acp-permission")).toHaveAttribute(
      "data-activity-state",
      "failed",
    );
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

  it("shows a decided permission as a compact activity row with expandable detail", () => {
    const { container } = render(
      <PermissionItem item={permission("resolvedAllowed")} />,
    );
    const row = container.querySelector(".acp-permission--settled");
    const disclosure = row?.querySelector("details");
    expect(row).not.toBeNull();
    expect(disclosure).not.toBeNull();
    expect(disclosure).not.toHaveAttribute("open");
    expect(disclosure?.querySelector("summary")).toHaveTextContent(
      "Write requested · Allowed",
    );
    fireEvent.click(disclosure!.querySelector("summary")!);
    expect(disclosure).toHaveAttribute("open");
    expect(disclosure).toHaveTextContent(
      "The harness requested a capability Sage does not allow.",
    );
  });
});
