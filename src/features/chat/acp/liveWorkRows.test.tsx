import {
  cleanup,
  fireEvent,
  render,
  screen,
  within,
} from "@testing-library/react";
import { afterEach, expect, it } from "vitest";

import type {
  AcpChatPage,
  AcpItem,
  TurnProjection,
  TurnSessionActivity,
} from "../../../domain/acp";
import { AcpChatPresentation } from "./AcpChatPresentation";
import { page, textItem } from "./acpProjection.fixture";

afterEach(cleanup);

const answer = (id: string) => ({ ...textItem(), id }) as unknown as AcpItem;

const PROGRESS = "Indexing synthetic docs (1 of 2)";

function mcpCall(
  toolState: "inProgress" | "completed",
): Extract<AcpItem, { kind: "toolCall" }> {
  return {
    ...textItem(),
    id: "mcp-call",
    kind: "toolCall",
    lifecycle: toolState === "inProgress" ? "started" : "completed",
    toolState,
    terminalAt: toolState === "inProgress" ? null : 2_000,
    payload: {
      inputSummary: null,
      outputSummary: null,
      presentation: {
        kind: "mcp",
        label: "index_fixture_docs",
        serverLabel: "MCP",
        toolLabel: "index_fixture_docs",
        argumentsSummary: null,
        resultSummary: null,
        progress: PROGRESS,
      },
    },
  } as Extract<AcpItem, { kind: "toolCall" }>;
}

const hooks: TurnSessionActivity[] = [
  { kind: "hook", label: "PreToolUse hook #1 (project)", state: "completed" },
  {
    kind: "hook",
    label: "PostToolUse hook #1 (project)",
    state: "failed",
    progress: "Synthetic lint hook exited with status 1",
  },
  { kind: "hook", label: "Stop hook #2 (user)", state: "running" },
];

function chat(
  state: TurnProjection["state"],
  turn: Partial<TurnProjection>,
): AcpChatPage {
  const projection = page(state);
  projection.turns = [
    {
      ...projection.turns[0],
      createdAt: 1_000,
      terminalAt: state === "running" ? null : 7_000,
      ...turn,
    },
  ];
  return projection;
}

function presentation(projection: AcpChatPage) {
  return (
    <AcpChatPresentation
      page={projection}
      hasEarlier={false}
      loadingEarlier={false}
      historyError={null}
      projectRoots={[]}
      retentionIncomplete={false}
    />
  );
}

it("shows a running MCP call's status line on its collapsed live work row and drops it when the call ends", () => {
  const { rerender } = render(
    presentation(chat("running", { items: [mcpCall("inProgress")] })),
  );
  const row = document.querySelector<HTMLDetailsElement>(".acp-tool-rollup")!;
  expect(row.open).toBe(false);
  expect(within(row).getByText(PROGRESS)).toBeVisible();
  expect(row.querySelector("summary")).toHaveAccessibleName(
    `index_fixture_docs, ${PROGRESS}, In progress`,
  );

  rerender(
    presentation(
      chat("running", {
        items: [mcpCall("inProgress"), answer("later")],
      }),
    ),
  );
  expect(screen.getByText(PROGRESS)).toBeVisible();

  rerender(presentation(chat("running", { items: [mcpCall("completed")] })));
  expect(screen.queryByText(PROGRESS)).toBeNull();
  expect(document.body).not.toHaveTextContent("1 of 2");
});

it("lists the Turn's hook runs in the live work list with their failure line", () => {
  render(
    presentation(
      chat("running", { items: [textItem()], sessionActivities: hooks }),
    ),
  );
  const list = screen.getByRole("list", { name: "Hooks" });
  const rows = within(list).getAllByRole("listitem");
  expect(rows.map((row) => row.getAttribute("data-activity-state"))).toEqual([
    "completed",
    "failed",
    "running",
  ]);
  // The state is part of each row's text for assistive technology.
  expect(rows[0]).toHaveTextContent(
    /^PreToolUse hook #1 \(project\), Completed$/,
  );
  expect(rows[1]).toHaveTextContent(
    /^PostToolUse hook #1 \(project\), Failed: Synthetic lint hook exited with status 1$/,
  );
  expect(within(rows[1]).getByText(", Failed:")).toHaveClass("sr-only");
  expect(rows[2]).toHaveTextContent(/^Stop hook #2 \(user\), Running$/);
  // A running row is not a live region of its own.
  expect(within(list).queryAllByRole("status")).toHaveLength(0);
});

it("keeps hook rows in a settled Turn behind its Worked disclosure", () => {
  const settledHooks = hooks.map((hook) =>
    hook.state === "running" ? { ...hook, state: "cancelled" as const } : hook,
  );
  render(
    presentation(
      chat("completed", {
        items: [answer("answer")],
        sessionActivities: settledHooks,
      }),
    ),
  );
  const worked = screen.getByText("Worked for 6s").closest("details")!;
  expect(screen.queryByRole("list", { name: "Hooks" })).toBeNull();
  fireEvent.click(worked.querySelector("summary")!);
  expect(worked.open).toBe(true);
  const list = screen.getByRole("list", { name: "Hooks" });
  expect(
    within(list).getByText("Synthetic lint hook exited with status 1"),
  ).toBeVisible();
  expect(
    within(list)
      .getAllByRole("listitem")
      .map((row) => row.getAttribute("data-activity-state")),
  ).toEqual(["completed", "failed", "cancelled"]);
});
