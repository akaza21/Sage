import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
} from "@testing-library/react";
import { useRef } from "react";
import { afterEach, expect, it, vi } from "vitest";

import type { AcpItem, TurnProjection } from "../../domain/acp";
import { acpDesktop } from "../../services/acpDesktop";

const chunk = vi.hoisted(() => {
  let fail: (error: Error) => void = () => undefined;
  const gate = new Promise<never>((_resolve, reject) => {
    fail = reject;
  });
  return { gate, fail: (error: Error) => fail(error) };
});
vi.mock("./acp/PermissionPrompt", async () => {
  await chunk.gate;
  return {};
});

import { clearApprovalDecisions, useApprovalSlot } from "./useApprovalSlot";

afterEach(() => {
  cleanup();
  clearApprovalDecisions();
  vi.restoreAllMocks();
});

const item = {
  id: "permission-item",
  projectId: "project-1",
  chatId: "chat-1",
  sessionId: "session-1",
  turnId: "turn-1",
  runtimeEpoch: "1",
  kind: "permissionRequest",
  toolState: "waitingForApproval",
  payload: {
    requestId: "request-1",
    state: "received",
    label: "Permission request",
    summary: "Run a command",
    offeredOptionKinds: ["allow_once", "reject_once"],
    category: "execute",
  },
} as unknown as AcpItem;

function Pane({ onCancelTurn }: { onCancelTurn: () => void }) {
  const composerRef = useRef<HTMLTextAreaElement>(null);
  const approval = useApprovalSlot(
    { id: "turn-1", items: [item] } as unknown as TurnProjection,
    "chat-1",
    composerRef,
    { onCancelTurn },
  );
  return (
    <>
      {approval.slot}
      <textarea
        ref={composerRef}
        aria-label="Message agent"
        hidden={approval.awaiting}
      />
    </>
  );
}

it("keeps Deny and Stop while the card loads, and restores the composer when its chunk fails", async () => {
  const resolve = vi
    .spyOn(acpDesktop, "resolvePermissionDecision")
    .mockRejectedValueOnce(new Error("refused"))
    .mockResolvedValue({} as never);
  const cancel = vi.fn();
  vi.spyOn(console, "error").mockImplementation(() => undefined);
  render(<Pane onCancelTurn={cancel} />);
  expect(screen.getByText("Loading the approval…")).toBeVisible();
  expect(
    screen.queryByRole("textbox", { name: "Message agent" }),
  ).not.toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "Stop turn" }));
  expect(cancel).toHaveBeenCalledOnce();
  const deny = screen.getByRole("button", { name: "Deny" });
  fireEvent.click(deny);
  // In flight: a second press sends nothing more.
  expect(deny).toBeDisabled();
  fireEvent.click(deny);
  expect(resolve).toHaveBeenCalledOnce();
  expect(resolve).toHaveBeenCalledWith(
    expect.objectContaining({ requestId: "request-1", decision: "deny" }),
  );
  expect(await screen.findByRole("alert")).toHaveTextContent(
    "The denial could not be sent",
  );
  fireEvent.click(deny);
  expect(resolve).toHaveBeenCalledTimes(2);
  await act(async () => {
    chunk.fail(new Error("chunk failed"));
    await new Promise((settle) => setTimeout(settle, 0));
  });
  expect(await screen.findByRole("alert")).toHaveTextContent(
    "The approval could not load",
  );
  expect(screen.getByRole("textbox", { name: "Message agent" })).toBeVisible();
});
