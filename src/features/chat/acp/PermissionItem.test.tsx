import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import type { AcpItem } from "../../../domain/acp";
import { acpDesktop } from "../../../services/acpDesktop";
import { PermissionItem } from "./PermissionItem";
import { getPermissionStatus } from "./statusSemantics";

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
  it.each([
    ["received", "Waiting for approval", "Waiting for approval", false],
    ["asked", "Asking again", "Asking again", false],
    ["allowPending", "Allowing", "Allowing", false],
    ["allowCommitted", "Allowing", "Allowing", false],
    ["declinePending", "Declining", "Declining", false],
    ["declineCommitted", "Declined", "Declined", false],
    ["responseQueued", "Responding", "Responding", false],
    ["resolvedAllowed", "Allowed", "Permission allowed", true],
    ["resolvedDeclined", "Declined", "Permission declined", true],
    ["cancelled", "Cancelled", "Permission cancelled", true],
  ] as const)(
    "preserves the %s status and accessibility",
    (state, visual, accessible, terminal) => {
      expect(getPermissionStatus(permission(state))).toEqual({
        visual,
        accessible,
        terminal,
      });
      expect(getPermissionStatus(permission(state, true))).toEqual({
        visual: "Failed closed",
        accessible: "Permission failed closed",
        terminal: true,
      });
    },
  );

  it("leaves an awaited decision to the composer-slot prompt", () => {
    for (const state of ["received", "asked"] as const) {
      const { container, unmount } = render(
        <PermissionItem item={permission(state)} />,
      );
      expect(container).toBeEmptyDOMElement();
      unmount();
    }
    render(<PermissionItem item={permission("allowPending")} />);
    expect(
      screen.getByRole("group", { name: "Permission request" }),
    ).toHaveTextContent("Write requested · Allowing");
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
      <PermissionItem item={permission("declinePending")} />,
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
