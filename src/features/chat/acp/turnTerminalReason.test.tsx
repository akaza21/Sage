import { cleanup, render, screen, within } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";

import type { TurnTerminalReason } from "../../../domain/acp";
import { settleLazyModules } from "../../../test/settleLazyModules";
import { AcpChatProjection } from "./AcpChatProjection";
import { page } from "./acpProjection.fixture";
import { turnTerminalReasonLabels } from "./statusSemantics";

async function renderTurn(
  state: "failed" | "cancelled" | "completed",
  reason?: {
    kind: TurnTerminalReason;
    message?: string;
    remediation?: string;
  },
) {
  const result = render(
    <AcpChatProjection page={page(state, reason)} active />,
  );
  await settleLazyModules();
  return result;
}

function header(): HTMLElement {
  const found = document.querySelector(".acp-turn__header");
  if (!(found instanceof HTMLElement))
    throw new Error("Turn header was unavailable");
  return found;
}

afterEach(cleanup);

describe("turn terminal reason", () => {
  it("names every reason the decoder can admit", () => {
    const kinds: TurnTerminalReason[] = [
      "authRequired",
      "harnessExited",
      "protocolError",
      "cancelledByUser",
      "refusedVersion",
      "timeout",
      "other",
    ];
    expect(Object.keys(turnTerminalReasonLabels).sort()).toEqual(
      [...kinds].sort(),
    );
    for (const kind of kinds)
      expect(turnTerminalReasonLabels[kind].length).toBeGreaterThan(0);
  });

  it("keeps the failed status chip separate from the single failure cause", async () => {
    await renderTurn("failed", { kind: "harnessExited" });

    const chip = within(header()).getByRole("status");
    expect(chip).toHaveTextContent("Failed");
    expect(header().querySelector("[data-turn-reason]")).toBeNull();
    const failure = screen.getByRole("group", { name: "Turn failed" });
    const reason = within(failure).getByText("Harness exited");
    expect(screen.getAllByText("Harness exited")).toHaveLength(1);
    expect(reason).toHaveAttribute("data-turn-reason-message", "");
    expect(chip.compareDocumentPosition(reason)).toBe(
      Node.DOCUMENT_POSITION_FOLLOWING,
    );
  });

  it("renders a cancelled turn reason in the user's terms", async () => {
    await renderTurn("cancelled", { kind: "cancelledByUser" });

    expect(within(header()).getByText("Cancelled by you")).toHaveAttribute(
      "data-turn-reason",
      "cancelledByUser",
    );
  });

  it("renders nothing when the turn carries no reason", async () => {
    await renderTurn("failed");

    expect(header().querySelector("[data-turn-reason]")).toBeNull();
    expect(within(header()).getByRole("status")).toHaveTextContent("Failed");
  });

  it("shows a harness message in the failed-turn row", async () => {
    await renderTurn("failed", {
      kind: "protocolError",
      message: "Unsupported sessionUpdate variant",
    });

    expect(header().querySelector("[data-turn-reason]")).toBeNull();
    const failure = screen.getByRole("group", { name: "Turn failed" });
    const detail = within(failure).getByText(
      "Unsupported sessionUpdate variant",
    );
    expect(detail).toHaveAttribute("data-turn-reason-message", "");
    expect(failure.querySelector("[data-turn-reason-remediation]")).toBeNull();
  });

  it("shows the Sage remediation for a sign-in failure", async () => {
    await renderTurn("failed", {
      kind: "authRequired",
      remediation: "Sign in by running claude auth login.",
    });

    expect(header().querySelector("[data-turn-reason]")).toBeNull();
    expect(screen.getAllByText("Not signed in")).toHaveLength(1);
    const failure = screen.getByRole("group", { name: "Turn failed" });
    const remediation = within(failure).getByText(
      "Sign in by running claude auth login.",
    );
    expect(remediation).toHaveAttribute("data-turn-reason-remediation", "");
    expect(within(failure).getByText("Not signed in")).toHaveAttribute(
      "data-turn-reason-message",
      "",
    );
  });

  it("keeps a message and a remediation as two separate blocks", async () => {
    await renderTurn("failed", {
      kind: "authRequired",
      message: "authentication required",
      remediation: "Sign in with your Claude account",
    });

    const failure = screen.getByRole("group", { name: "Turn failed" });
    const message = within(failure).getByText("authentication required");
    const remediation = within(failure).getByText(
      "Sign in with your Claude account",
    );
    expect(message).not.toBe(remediation);
    expect(message.compareDocumentPosition(remediation)).toBe(
      Node.DOCUMENT_POSITION_FOLLOWING,
    );
    expect(failure.textContent).not.toContain("—");
  });

  it("offers no reason detail when the turn carries neither text", async () => {
    await renderTurn("failed", { kind: "timeout" });

    expect(header().querySelector("[data-turn-reason-message]")).toBeNull();
    expect(header().querySelector("[data-turn-reason-remediation]")).toBeNull();
    expect(header().querySelector("[data-turn-reason]")).toBeNull();
    expect(screen.getAllByText("Timed out")).toHaveLength(1);
    expect(
      within(screen.getByRole("group", { name: "Turn failed" })).getByText(
        "Timed out",
      ),
    ).toBeVisible();
  });
});
