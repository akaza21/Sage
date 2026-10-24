import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { useRef } from "react";
import { afterEach, expect, it, vi } from "vitest";

import type { AcpItem, TurnProjection } from "../../domain/acp";
import { acpDesktop } from "../../services/acpDesktop";

const chunk = vi.hoisted(() => {
  let open: () => void = () => undefined;
  const gate = new Promise<void>((resolve) => {
    open = resolve;
  });
  return { gate, open: () => open() };
});
vi.mock("./acp/PermissionPrompt", async (importOriginal) => {
  await chunk.gate;
  return importOriginal();
});

import { clearApprovalDecisions, useApprovalSlot } from "./useApprovalSlot";

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
  clearApprovalDecisions();
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

function Pane() {
  const composerRef = useRef<HTMLTextAreaElement>(null);
  const approval = useApprovalSlot(
    { id: "turn-1", items: [item] } as unknown as TurnProjection,
    "chat-1",
    composerRef,
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

it("carries a fallback Deny into the loaded card: Allow is never focused for that request", async () => {
  let settle: () => void = () => undefined;
  const resolve = vi
    .spyOn(acpDesktop, "resolvePermissionDecision")
    .mockImplementation(
      () => new Promise((done) => (settle = () => done({} as never))),
    );
  render(<Pane />);
  fireEvent.click(screen.getByRole("button", { name: "Deny" }));
  expect(resolve).toHaveBeenCalledOnce();
  await act(async () => {
    chunk.open();
    await new Promise((wait) => setTimeout(wait, 0));
  });
  const allow = await screen.findByRole("button", { name: /Allow once/ });
  expect(allow).not.toHaveFocus();
  // The Deny is still in flight: the card shows it, and sends nothing more.
  await waitFor(() => expect(allow).toBeDisabled());
  fireEvent.click(screen.getByRole("button", { name: /Deny/ }));
  expect(resolve).toHaveBeenCalledOnce();
  await act(async () => settle());
  expect(allow).toBeDisabled();
  expect(allow).not.toHaveFocus();
});
