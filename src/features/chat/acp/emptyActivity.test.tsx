import {
  cleanup,
  fireEvent,
  render,
  screen,
  within,
} from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";

import type { AcpItem, TurnProjection } from "../../../domain/acp";
import { AcpChatPresentation } from "./AcpChatPresentation";
import { page, textItem } from "./acpProjection.fixture";

afterEach(cleanup);

function renderTurn(state: TurnProjection["state"], items: AcpItem[]) {
  const projection = page(state);
  projection.turns = [
    {
      ...projection.turns[0],
      createdAt: 1_000,
      terminalAt:
        state === "running" || state === "waitingApproval" ? null : 1_000,
      items,
    },
  ];
  return render(
    <AcpChatPresentation
      page={projection}
      hasEarlier={false}
      loadingEarlier={false}
      historyError={null}
      projectRoots={[]}
      retentionIncomplete={false}
    />,
  );
}

describe("empty turn activity", () => {
  it.each([
    "received",
    "resolvedAllowed",
    "resolvedDeclined",
    "cancelled",
  ] as const)(
    "renders the actual %s permission row only when completed Worked opens",
    (state) => {
      const pending = state === "received";
      const permission = {
        ...textItem(),
        id: "permission-row",
        kind: "permissionRequest",
        lifecycle: pending ? "streaming" : "completed",
        toolState: pending ? "waitingForApproval" : "completed",
        payload: {
          requestId: "permission-row",
          state,
          label: "Read project files",
          summary: "Permission evidence",
          offeredOptionKinds: ["allow_once", "reject_once"],
        },
      } as unknown as AcpItem;
      renderTurn("completed", [permission, textItem()]);
      expect(
        screen.queryByRole("group", { name: "Permission request" }),
      ).toBeNull();
      const worked = screen.getByText("Worked for 0s").closest("details")!;
      fireEvent.click(worked.querySelector("summary")!);
      const row = screen.getByRole("group", { name: "Permission request" });
      expect(row).toBeVisible();
      expect(row).toHaveAttribute("data-acp-item-id", "permission-row");
      expect(within(row).getByText("Read project files")).toBeVisible();
      // A terminal Turn offers no decision, even for a request whose own
      // state never settled; it says so instead of asking.
      expect(within(row).queryByRole("button")).toBeNull();
      if (pending)
        expect(within(row).getByRole("status")).toHaveAccessibleName(
          "Permission not answered",
        );
      fireEvent.click(worked.querySelector("summary")!);
      expect(
        screen.queryByRole("group", { name: "Permission request" }),
      ).toBeNull();
    },
  );
  it("does not disclose blank progress as completed activity", () => {
    const progress = {
      ...textItem(),
      id: "blank-progress",
      ordinal: 1,
      payload: { role: "assistant", text: "   " },
    } as AcpItem;
    const answer = { ...textItem(), ordinal: 2 };
    const { container } = renderTurn("completed", [progress, answer]);
    expect(screen.getByText("Normalized answer")).toBeVisible();
    expect(container.querySelector("details.chat-transcript-work")).toBeNull();
    expect(screen.getByText("Worked for 0s")).toBeVisible();
  });

  it.each([
    "completed",
    "cancelled",
    "interrupted",
    "failed",
    "deliveryUnknown",
  ] as const)("does not disclose empty work for a %s turn", (state) => {
    const { container } = renderTurn(state, []);
    expect(container.querySelector(".chat-transcript-work")).toBeNull();
    expect(screen.queryByText("Worked for 0s")).toBeNull();
  });

  it("keeps a real zero-duration answer without an empty disclosure", () => {
    const answer = textItem();
    answer.payload.text =
      "This project is ready. Choose a harness and begin a persistent chat.";
    const { container } = renderTurn("completed", [answer]);
    expect(screen.getByText(answer.payload.text)).toBeVisible();
    expect(container.querySelector("details.chat-transcript-work")).toBeNull();
    expect(screen.getByText("Worked for 0s")).toBeVisible();
  });

  it("keeps Working visible while waiting for the first token", () => {
    const { container } = renderTurn("running", []);
    expect(
      container.querySelector(".chat-transcript-work--running"),
    ).toBeVisible();
    expect(container.querySelector("details.chat-transcript-work")).toBeNull();
  });

  it("retains the zero-duration disclosure when there is renderable activity", () => {
    const notice = {
      ...textItem(),
      kind: "compactionNotice",
      id: "compaction-1",
      lifecycle: "completed",
      payload: {},
    } as unknown as AcpItem;
    const { container } = renderTurn("completed", [notice, textItem()]);
    expect(screen.getByText("Worked for 0s")).toBeVisible();
    expect(
      container.querySelector("details.chat-transcript-work"),
    ).not.toHaveAttribute("open");
    expect(screen.queryByText("Context automatically compacted")).toBeNull();
    const work = container.querySelector<HTMLDetailsElement>(
      "details.chat-transcript-work",
    )!;
    work.open = true;
    fireEvent(work, new Event("toggle"));
    expect(
      container.querySelector(".chat-transcript-work__groups"),
    ).toHaveTextContent("Context automatically compacted");
  });

  it.each(["waitingApproval", "completed"] as const)(
    "keeps a pending approval out of a %s turn's transcript actions",
    (state) => {
      const permission = {
        ...textItem(),
        kind: "permissionRequest",
        lifecycle: "running",
        toolState: "waitingForApproval",
        payload: {
          requestId: "permission-1",
          state: "received",
          label: "Read file",
          summary: "Allow reading the file",
          offeredOptionKinds: ["allow_once", "reject_once"],
        },
      } as unknown as AcpItem;
      renderTurn(state, [permission]);
      if (state === "completed") {
        const work = screen.getByText("Worked for 0s").closest("details")!;
        expect(work).not.toHaveAttribute("open");
        expect(screen.queryByRole("button", { name: "Approve" })).toBeNull();
        work.open = true;
        fireEvent(work, new Event("toggle"));
      }
      if (state === "completed") {
        // The runtime never commits a terminal Turn with a pending request,
        // so a terminal Turn shows the request as unanswered, not actionable.
        const row = screen.getByRole("group", { name: "Permission request" });
        expect(row).toBeVisible();
        expect(within(row).queryByRole("button")).toBeNull();
        expect(within(row).getByRole("status")).toHaveAccessibleName(
          "Permission not answered",
        );
      } else {
        // A live Turn answers the request in the composer slot, so the
        // transcript keeps no card.
        expect(
          screen.queryByRole("group", { name: "Permission request" }),
        ).toBeNull();
      }
      expect(
        screen.queryByRole("button", { name: /Approve|Allow/ }),
      ).toBeNull();
    },
  );
});
