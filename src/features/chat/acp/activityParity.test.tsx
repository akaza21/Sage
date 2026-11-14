import {
  cleanup,
  fireEvent,
  render,
  screen,
  within,
} from "@testing-library/react";
import { afterEach, expect, it } from "vitest";
import type { AcpItem } from "../../../domain/acp";
import { PrimaryActivities } from "./PrimaryActivities";
import { settleLazyModules } from "../../../test/settleLazyModules";
import { parityTool } from "./activityParity.fixture";
import { AcpChatPresentation } from "./AcpChatPresentation";
import { page, textItem } from "./acpProjection.fixture";
import { projectTaskResources } from "../../../workbench/agentArtifactTabPresentation";
import { ToolItem } from "./ToolItem";

afterEach(cleanup);

it("offers no dead expandDetails prop on Worked rows", () => {
  // Type-level: tsc fails if the public owner accepts the removed prop.
  const { container } = render(
    <PrimaryActivities
      items={[]}
      interactive
      renderItem={() => null}
      // @ts-expect-error expandDetails was removed: nothing passed it.
      expandDetails
    />,
  );
  expect(container).toBeEmptyDOMElement();
  type RollupProps = Parameters<typeof PrimaryActivities>[0];
  const rollupProps: Omit<RollupProps, "items" | "renderItem"> = {
    interactive: true,
    // @ts-expect-error expandDetails was removed: nothing passed it.
    expandDetails: true,
  };
  expect(rollupProps.interactive).toBe(true);
});

it("keeps execute members of mixed Worked activity behind individual Ran rows", () => {
  const { container } = render(
    <PrimaryActivities
      items={[
        parityTool("read"),
        parityTool("execute", 2),
        parityTool("execute", 3),
      ]}
      interactive
      renderItem={() => null}
    />,
  );
  const rollup = container.querySelector("details")!;
  expect(rollup).not.toHaveAttribute("open");
  rollup.open = true;
  fireEvent(rollup, new Event("toggle"));
  expect(rollup).toHaveAttribute("open");
  expect(rollup.querySelector("summary")).toHaveTextContent(
    "Read a file, ran commands",
  );
  expect(screen.queryByText("Shell")).toBeNull();
  const [read, ...commands] = rollup.querySelectorAll<HTMLDetailsElement>(
    ".acp-tool-rollup__items > details",
  );
  expect(read).toHaveTextContent("Read example-1.ts");
  expect(commands).toHaveLength(2);
  for (const command of commands) {
    expect(command).toHaveTextContent("Ran git status --short");
    expect(command).not.toHaveAttribute("open");
  }
  commands[0].open = true;
  fireEvent(commands[0], new Event("toggle"));
  expect(screen.getAllByText("Shell")).toHaveLength(1);
});

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
    const commands = disclosure.querySelectorAll<HTMLDetailsElement>("details");
    expect(commands).toHaveLength(2);
    for (const command of commands) {
      expect(command).not.toHaveAttribute("open");
      command.open = true;
      fireEvent(command, new Event("toggle"));
    }
    const cards = screen.getAllByRole("group", { name: "git status --short" });
    expect(within(cards[0]).getByRole("status")).toHaveTextContent("✓ Success");
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
  ["read", "Reading example-1.ts"],
  ["edit", "Editing example-1.ts"],
] as const)(
  "renders one present-tense %s label as a shimmer without a spinner",
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
    expect(summary.querySelector(".acp-tool-running")).toBeNull();
    expect(summary.querySelector(".acp-live-shimmer")).toHaveTextContent(label);
    expect(summary).toHaveAttribute("aria-label", `${label}, In progress`);
  },
);

it("names a group of reads without counting its members", () => {
  render(
    <PrimaryActivities
      items={[parityTool("read"), parityTool("read", 2), parityTool("read", 3)]}
      interactive
      renderItem={() => null}
    />,
  );
  expect(screen.getByText("Read files")).toBeVisible();
  expect(screen.queryByText(/Read \d/)).toBeNull();
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
  expect(resources.recentAgents.map((agent) => agent.status)).toEqual([
    "working",
    "working",
  ]);
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
    ).toHaveTextContent("✓ Success");
    expect(screen.getByLabelText("Command line")).toHaveTextContent(
      kind === "execute" ? "$ git status --short" : "src/example-1.ts",
    );
  },
);

it("expanding Worked shows the read group closed; opening it lists one row per file", () => {
  const kind = "read";
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
  const rowLabels = () =>
    [
      ...document.querySelectorAll(
        ".acp-tool-rollup__items > details .acp-tool-rollup__label",
      ),
    ].map((label) => label.textContent);
  expect(rowLabels()).toEqual([]);
  const work = screen.getByText(/^Worked for /).closest("details")!;
  work.open = true;
  fireEvent(work, new Event("toggle"));
  const group = work.nextElementSibling!.querySelector("details")!;
  expect(group).not.toHaveAttribute("open");
  expect(rowLabels()).toEqual([]);
  group.open = true;
  fireEvent(group, new Event("toggle"));
  expect(rowLabels()).toEqual([
    "Read example-1.ts",
    "Read example-2.ts",
    "Read example-3.ts",
  ]);
  expect(screen.queryByLabelText("Command line")).toBeNull();
  work.open = false;
  fireEvent(work, new Event("toggle"));
  expect(rowLabels()).toEqual([]);
  work.open = true;
  fireEvent(work, new Event("toggle"));
  expect(rowLabels()).toEqual([]);
});

it("expanding Worked lists three collapsed per-command Ran rows", () => {
  const projection = page("completed");
  projection.turns[0].items = [
    parityTool(),
    parityTool("execute", 2),
    parityTool("execute", 3),
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
  const work = screen.getByText(/^Worked for /).closest("details")!;
  expect(work).not.toHaveAttribute("open");
  expect(screen.queryByText("Shell")).toBeNull();
  work.open = true;
  fireEvent(work, new Event("toggle"));
  const group = work.nextElementSibling!.querySelector("details")!;
  expect(group).not.toHaveAttribute("open");
  group.open = true;
  fireEvent(group, new Event("toggle"));
  const commands =
    work.nextElementSibling!.querySelectorAll<HTMLDetailsElement>(
      ".acp-tool-rollup__items > details",
    );
  expect(commands).toHaveLength(3);
  for (const command of commands) {
    expect(command).toHaveTextContent("Ran git status --short");
    expect(command).not.toHaveAttribute("open");
  }
  expect(screen.queryByText("Shell")).toBeNull();
  commands[1].open = true;
  fireEvent(commands[1], new Event("toggle"));
  expect(screen.getAllByText("Shell")).toHaveLength(1);
  expect(screen.getAllByLabelText("Command line")).toHaveLength(1);
  expect(screen.getAllByText("fixture output")).toHaveLength(1);
});

it("keeps a steered message visible once the turn's work is folded away", async () => {
  const at = (id: string, ordinal: number, payload: object, kind = "text") =>
    ({ ...textItem(), id, kind, ordinal, payload }) as unknown as AcpItem;
  const next = page("completed");
  next.turns[0].items = [
    at("prompt-1", 1, { role: "user", text: "Fix the build" }),
    at("progress-1", 2, { role: "assistant", text: "Checking the notes" }),
    at("steered-1", 3, { text: "Also update the README" }, "userPrompt"),
    at("answer-1", 4, { role: "assistant", text: "Both are done" }),
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
  await settleLazyModules();
  expect(screen.getByText("Also update the README")).toBeInTheDocument();
  expect(screen.queryByText("Checking the notes")).toBeNull();
  expect(screen.getByText("Both are done")).toBeInTheDocument();
  fireEvent.click(screen.getByText(/^Worked/u));
  expect(screen.getByText("Checking the notes")).toBeInTheDocument();
  expect(screen.getAllByText("Also update the README")).toHaveLength(1);
});
