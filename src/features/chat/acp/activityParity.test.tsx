import {
  cleanup,
  fireEvent,
  render,
  screen,
  within,
} from "@testing-library/react";
import { afterEach, expect, it } from "vitest";
import { PrimaryActivities } from "./PrimaryActivities";
import { parityTool } from "./activityParity.fixture";
import { AcpChatPresentation } from "./AcpChatPresentation";
import { page, textItem } from "./acpProjection.fixture";
import { projectTaskResources } from "../../../workbench/agentArtifactTabPresentation";
import { ToolItem } from "./ToolItem";

afterEach(cleanup);

it("keeps the diff-only live editing row to one label and inline spinner", () => {
  const projection = page("running");
  const turn = projection.turns[0];
  turn.items = [
    {
      ...parityTool("edit", 1, "inProgress"),
      projectId: projection.projectId,
      chatId: projection.chatId,
      sessionId: turn.sessionId,
      turnId: turn.id,
      runtimeEpoch: turn.runtimeEpoch,
      kind: "fileChangeDiff",
      lifecycle: "streaming",
      toolState: null,
      payload: { changes: [], truncated: false },
    },
  ];
  const { container } = render(
    <AcpChatPresentation
      page={projection}
      hasEarlier={false}
      loadingEarlier={false}
      historyError={null}
      projectRoots={[]}
      retentionIncomplete={false}
    />,
  );
  const row = container.querySelector<HTMLElement>(".acp-editing-files")!;
  expect(within(row).getByText("Editing files")).toBeVisible();
  expect(within(row).getByRole("status", { name: "Running" })).toBeVisible();
  expect(row).toHaveTextContent(/^Editing files$/);
});

it.each(["failed", "cancelled", "declined"] as const)(
  "identifies the %s member inside compact grouped command details",
  (state) => {
    const failed = { ...parityTool("execute", 2), toolState: state };
    const { container } = render(
      <PrimaryActivities
        items={[parityTool(), failed]}
        interactive
        renderItem={() => null}
      />,
    );
    const disclosure = container.querySelector("details")!;
    disclosure.open = true;
    fireEvent(disclosure, new Event("toggle"));
    const cards = screen.getAllByRole("group", { name: "git status --short" });
    expect(within(cards[0]).getByRole("status")).toHaveTextContent("Completed");
    expect(within(cards[1]).getByRole("status")).toHaveTextContent(
      state === "failed"
        ? "Failed"
        : state === "cancelled"
          ? "Cancelled"
          : "Declined",
    );
    expect(cards[1].querySelector("header")).toBeNull();
  },
);

it("preserves the same emitted command and output fallback inside and outside rollups", () => {
  const command = parityTool();
  command.payload.outputSummary = null;
  if (command.payload.presentation.kind === "execute")
    command.payload.presentation.outputTail = "retained command tail";
  const { rerender } = render(<ToolItem item={command} />);
  expect(screen.getByLabelText("Command output")).toHaveTextContent(
    "retained command tail",
  );
  rerender(<ToolItem item={command} compact />);
  expect(screen.getByLabelText("Command output")).toHaveTextContent(
    "retained command tail",
  );
  expect(screen.getByLabelText("Command line")).toHaveTextContent(
    "$ git status --short",
  );
});

it.each([
  ["execute", "Running git status --short"],
  ["read", "Reading 1 file"],
  ["edit", "Editing 1 file"],
] as const)(
  "renders one present-tense %s label with an inline spinner",
  (kind, label) => {
    const { container } = render(
      <PrimaryActivities
        items={[parityTool(kind, 1, "inProgress")]}
        interactive
        renderItem={() => null}
      />,
    );
    const summary = container.querySelector("summary")!;
    expect(within(summary).getByText(label)).toBeVisible();
    expect(within(summary).queryByText("Running", { exact: true })).toBeNull();
    expect(
      summary.querySelector(".acp-tool-rollup__summary-copy .acp-tool-running"),
    ).not.toBeNull();
    expect(screen.getByRole("status", { name: "Running" })).toBeInTheDocument();
  },
);

it("counts three distinct read members instead of repeating singleton labels", () => {
  render(
    <PrimaryActivities
      items={[parityTool("read"), parityTool("read", 2), parityTool("read", 3)]}
      interactive
      renderItem={() => null}
    />,
  );
  expect(screen.getByText("Read 3 files")).toBeVisible();
  expect(screen.queryByText("Read 1 file")).toBeNull();
});

it("names pending task observations as started working and keeps grouped/header counts consistent", () => {
  const tasks = ["Reviewer", "Builder"].map((label, index) => ({
    ...parityTool("execute", index + 1, "pending"),
    payload: {
      inputSummary: null,
      presentation: {
        kind: "subagent" as const,
        label,
        activityId: null,
        taskLifecycle: true,
      },
    },
  }));
  const projection = page("running");
  projection.turns[0].items = tasks;
  render(
    <PrimaryActivities items={tasks} interactive renderItem={() => null} />,
  );
  expect(
    screen.getByText("Reviewer and Builder started working"),
  ).toBeVisible();
  expect(screen.queryByText(/pending/)).toBeNull();
  const resources = projectTaskResources(projection, projection.chatId);
  expect(resources.workingAgents).toBe(2);
  expect(resources.recentAgents.map((agent) => agent.label)).toEqual([
    "Reviewer",
    "Builder",
  ]);
});

it.each(["execute", "read"] as const)(
  "opens %s directly into a compact command/output card",
  (kind) => {
    const { container } = render(
      <PrimaryActivities
        items={[parityTool(kind)]}
        interactive
        renderItem={() => null}
      />,
    );
    const disclosure = container.querySelector("details")!;
    disclosure.open = true;
    fireEvent(disclosure, new Event("toggle"));
    expect(
      screen.getByText(kind === "execute" ? "Shell" : "Read"),
    ).toBeVisible();
    expect(screen.getByText("fixture output")).toBeVisible();
    expect(
      container.querySelector(".acp-tool-rollup__items header"),
    ).toBeNull();
    expect(
      container.querySelector(".acp-tool-rollup__items details"),
    ).toBeNull();
    expect(
      within(container.querySelector(".acp-tool-rollup__items")!).getByRole(
        "status",
      ),
    ).toHaveTextContent("Completed");
    expect(screen.getByLabelText("Command line")).toHaveTextContent(
      kind === "execute" ? "$ git status --short" : "src/example-1.ts",
    );
  },
);

it.each(["execute", "read"] as const)(
  "expanding Worked shows every %s detail without another disclosure click",
  (kind) => {
    const projection = page("completed");
    projection.turns[0].items = [
      parityTool(kind),
      parityTool(kind, 2),
      parityTool(kind, 3),
      { ...textItem(), ordinal: 5 },
    ];
    render(
      <AcpChatPresentation
        page={projection}
        hasEarlier={false}
        loadingEarlier={false}
        historyError={null}
        projectRoots={[]}
        retentionIncomplete={false}
      />,
    );
    const title = kind === "execute" ? "Shell" : "Read";
    expect(screen.queryByText(title, { exact: true })).toBeNull();
    const work = screen.getByText(/^Worked for /).closest("details")!;
    work.open = true;
    fireEvent(work, new Event("toggle"));
    expect(screen.getAllByText(title, { exact: true })).toHaveLength(3);
    expect(screen.getAllByLabelText("Command line")).toHaveLength(3);
    expect(screen.getAllByText("fixture output")).toHaveLength(3);
    expect(work.querySelector("details")).toHaveAttribute("open");
    work.open = false;
    fireEvent(work, new Event("toggle"));
    expect(screen.queryByText(title, { exact: true })).toBeNull();
    work.open = true;
    fireEvent(work, new Event("toggle"));
    expect(screen.getAllByText(title, { exact: true })).toHaveLength(3);
  },
);
