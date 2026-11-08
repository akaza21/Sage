import { act, cleanup, render, screen } from "@testing-library/react";
import { useRef } from "react";
import { afterEach, expect, it, vi } from "vitest";

import type { AcpItem, TurnProjection } from "../../domain/acp";
import { logCardArrival } from "./acp/approvalCard";
import { clearApprovalDecisions, useApprovalSlot } from "./useApprovalSlot";

afterEach(() => {
  cleanup();
  clearApprovalDecisions();
  vi.restoreAllMocks();
});

function permission(
  state: "received" | "allowPending" = "received",
  revision = "1",
): AcpItem {
  return {
    id: "bridge-write-item",
    projectId: "project-1",
    chatId: "chat-1",
    sessionId: "session-1",
    turnId: "turn-1",
    runtimeEpoch: "1",
    ordinal: 1,
    origin: "primary",
    activityId: null,
    lifecycle: "streaming",
    revision,
    createdAt: 1,
    updatedAt: 1,
    terminalAt: null,
    kind: "permissionRequest",
    toolState: "waitingForApproval",
    payload: {
      requestId: "request-bridge-write",
      state,
      label: "Permission request",
      summary: "Allow Pi to update src/bridge-note.txt?",
      offeredOptionKinds: ["allow_once", "reject_once"],
      category: "edit",
    },
  } as AcpItem;
}

function Pane({ items }: { items: AcpItem[] }) {
  const composerRef = useRef<HTMLTextAreaElement>(null);
  const approval = useApprovalSlot(
    { id: "turn-1", items } as unknown as TurnProjection,
    "chat-1",
    composerRef,
  );
  return (
    <section className="chat-surface" aria-label="Pane">
      {approval.slot}
      <textarea ref={composerRef} aria-label="Message agent" />
    </section>
  );
}

// This file loads the approval chunk for the first time, so the card goes
// through its real lazy load here.
it("shows one request as exactly one card, with one item id, from first load to Allow", async () => {
  const info = vi.spyOn(console, "info").mockImplementation(() => undefined);
  const warn = vi.spyOn(console, "warn").mockImplementation(() => undefined);
  const cards = () => [
    ...document.querySelectorAll<HTMLElement>("section.acp-permission-prompt"),
  ];
  let most = 0;
  const ids = new Set<string>();
  const observer = new MutationObserver(() => {
    most = Math.max(most, cards().length);
    for (const card of cards())
      if (card.dataset.acpItemId) ids.add(card.dataset.acpItemId);
  });
  observer.observe(document.body, { childList: true, subtree: true });

  const view = render(<Pane items={[permission()]} />);
  // The loading fallback holds the slot until the chunk arrives.
  expect(cards()).toHaveLength(1);
  const card = await screen.findByRole("group", {
    name: "Permission request",
    busy: false,
  });
  expect(card).toHaveAttribute("data-acp-item-id", "bridge-write-item");

  // A re-projection of the same request keeps the same card.
  view.rerender(<Pane items={[permission("received", "2")]} />);
  expect(cards()).toEqual([card]);

  // Allow settles it; the slot hands back to the composer.
  view.rerender(<Pane items={[permission("allowPending", "3")]} />);
  await act(async () => undefined);
  observer.disconnect();
  expect(cards()).toHaveLength(0);
  expect(most).toBe(1);
  expect([...ids]).toEqual(["bridge-write-item"]);
  expect(
    info.mock.calls.filter(([message]) =>
      String(message).startsWith("approval card:"),
    ),
  ).toEqual([
    [
      "approval card: new request",
      {
        itemId: "bridge-write-item",
        requestId: "request-bridge-write",
        arrival: "new request",
        live: ["bridge-write-item"],
      },
    ],
  ]);
  expect(warn).not.toHaveBeenCalled();
});

it("tells a remount, a re-projected request and a second request apart in the log", () => {
  const info = vi.spyOn(console, "info").mockImplementation(() => undefined);
  const arrive = (id: string, requestId: string) =>
    logCardArrival({ id, payload: { requestId } }, null);
  arrive("item-a", "request-log-1");
  arrive("item-a", "request-log-1");
  arrive("item-b", "request-log-1");
  arrive("item-c", "request-log-2");
  expect(info.mock.calls.map(([message]) => message)).toEqual([
    "approval card: new request",
    "approval card: remount of the same request",
    "approval card: same request re-projected (was item item-a)",
    "approval card: new request",
  ]);
});
