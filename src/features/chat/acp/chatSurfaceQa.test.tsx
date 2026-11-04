import { cleanup, render, screen, within } from "@testing-library/react";
import { afterEach, expect, it } from "vitest";
import { page, textItem } from "./acpProjection.fixture";
import { AcpChatPresentation } from "./AcpChatPresentation";
import { projectChatLiveSnapshot } from "./chatLiveSnapshot";
import { TurnFileActivity } from "./TurnFileActivity";
import { snapshotPage } from "../../../app/agentDiff.fixture";
import { settleLazyModules } from "../../../test/settleLazyModules";

afterEach(cleanup);

it("waits for the dock to mount without creating a duplicate inline file card", async () => {
  const next = snapshotPage();
  const presentation = (container: HTMLElement | null) => (
    <AcpChatPresentation
      page={next}
      hasEarlier={false}
      loadingEarlier={false}
      historyError={null}
      projectRoots={[]}
      retentionIncomplete={false}
      editSummaryContainer={container}
    />
  );
  const { rerender } = render(presentation(null));
  await settleLazyModules();
  expect(screen.queryByRole("region", { name: "Edited files" })).toBeNull();
  rerender(presentation(document.body));
  expect(
    await screen.findByRole("region", { name: "Files edited in this chat" }),
  ).toHaveTextContent("1 file changed");
  expect(screen.queryByRole("region", { name: "Edited files" })).toBeNull();
});

it("shows a pending permission as the chat status while a turn still reports running", () => {
  const next = page("running");
  next.turns[0].items = [
    {
      ...textItem(),
      kind: "permissionRequest",
      toolState: "waitingForApproval",
      payload: {
        requestId: "permission",
        state: "received",
        label: "Write file",
        summary: "Requires approval",
      },
    } as never,
  ];
  expect(projectChatLiveSnapshot(next.chatId, next)).toMatchObject({
    statusLabel: "Awaiting approval",
    waiting: true,
    working: false,
  });
});

it("retains the interrupted cause outside Worked", () => {
  const next = page("interrupted", {
    kind: "other",
    message: "File write denied by policy",
  });
  render(
    <AcpChatPresentation
      page={next}
      hasEarlier={false}
      loadingEarlier={false}
      historyError={null}
      projectRoots={[]}
      retentionIncomplete={false}
    />,
  );
  expect(screen.getByText("File write denied by policy")).toBeVisible();
});

it("identifies incomplete workspace tracking as a system notice", () => {
  const next = page("completed");
  next.turns[0].items = [];
  render(
    <TurnFileActivity page={next} turn={next.turns[0]} workspaceIncomplete />,
  );
  expect(
    screen.getByRole("status", { name: "Workspace tracking" }),
  ).toHaveTextContent("Workspace change tracking is incomplete for this turn.");
  expect(
    within(
      screen.getByRole("status", { name: "Workspace tracking" }),
    ).getByText("System:"),
  ).toBeVisible();
});

it("shows the denied tool's own explanation without requiring a terminal reason", () => {
  const next = page("interrupted");
  next.turns[0].items = [
    ...next.turns[0].items,
    {
      ...textItem(),
      id: "denied",
      kind: "toolCall",
      toolState: "declined",
      payload: {
        outputSummary: "Write blocked: read-only access",
        presentation: {
          kind: "other",
          label: "Write",
          summary: "Write",
          resources: [],
        },
      },
    } as never,
  ];
  render(
    <AcpChatPresentation
      page={next}
      hasEarlier={false}
      loadingEarlier={false}
      historyError={null}
      projectRoots={[]}
      retentionIncomplete={false}
    />,
  );
  expect(screen.getByText("Write blocked: read-only access")).toBeVisible();
});

it("shows the normalized retry progress and error during a live turn", () => {
  const next = page("running");
  next.turns[0].items = [
    ...next.turns[0].items,
    {
      ...textItem(),
      id: "retry",
      kind: "error",
      lifecycle: "failed",
      payload: {
        kind: "attempt",
        code: "Attempt failed (attempt 1/3)",
        message: "Server unavailable",
        recoverable: false,
      },
    } as never,
  ];
  render(
    <AcpChatPresentation
      page={next}
      hasEarlier={false}
      loadingEarlier={false}
      historyError={null}
      projectRoots={[]}
      retentionIncomplete={false}
    />,
  );
  expect(screen.getByText("Attempt failed (attempt 1/3)")).toBeVisible();
  expect(screen.getByText("Server unavailable")).toBeVisible();
});
