import { cleanup, fireEvent, render } from "@testing-library/react";
import { afterEach, expect, it } from "vitest";

import type { AcpItem, ToolPresentation } from "../../../domain/acp";
import { PrimaryActivities } from "./PrimaryActivities";
import type { ComponentProps } from "react";
import type { ToolRollup } from "./toolActivityRollup";
function ToolActivityRollup({
  rollup,
  ...props
}: Pick<
  ComponentProps<typeof PrimaryActivities>,
  "interactive" | "onOpenResource"
> & { rollup: ToolRollup }) {
  return (
    <PrimaryActivities
      {...props}
      items={rollup.items}
      renderItem={() => null}
    />
  );
}
import { WorkedTurnTerminal } from "./WorkedTurnTerminal";

afterEach(cleanup);

type ToolProjectionItem = Extract<AcpItem, { kind: "toolCall" }>;

function tool(
  presentation: ToolPresentation,
  toolState: ToolProjectionItem["toolState"] = "completed",
): ToolProjectionItem {
  return {
    id: `tool-${presentation.kind}`,
    projectId: "project-1",
    chatId: "chat-1",
    sessionId: "session-1",
    turnId: "turn-1",
    runtimeEpoch: "1",
    ordinal: 1,
    origin: "primary",
    activityId: null,
    lifecycle: toolState === "failed" ? "failed" : "completed",
    revision: "1",
    createdAt: 1,
    updatedAt: 2,
    terminalAt: 2,
    kind: "toolCall",
    toolState,
    payload: { presentation, inputSummary: null, outputSummary: null },
  } as ToolProjectionItem;
}

const mcp: ToolPresentation = {
  kind: "mcp",
  label: "search_fixture_docs",
  serverLabel: "fixture-docs",
  toolLabel: "search_fixture_docs",
  argumentsSummary: null,
  resultSummary: null,
};

it.each([
  ["mcp", mcp],
  ["webSearch", { kind: "webSearch", label: "Web search" } as const],
] as const)(
  "gives %s rows Codex's command glyph, not a plug (s5/f0405)",
  (kind, presentation) => {
    const { container } = render(
      <ToolActivityRollup
        rollup={{
          kind,
          commandActivity: "ordinary",
          items: [tool(presentation)],
        }}
        interactive
      />,
    );
    const icon = container.querySelector("summary svg[data-activity-icon]");
    expect(icon).toHaveClass("lucide-command");
    expect(icon).toHaveAttribute("fill", "none");
    expect(icon).toHaveAttribute("stroke", "currentColor");
    expect(icon?.querySelector("path")).not.toBeNull();
    expect(icon?.textContent).toBe("");
    expect(container.querySelector(".lucide-plug")).toBeNull();
  },
);

it("draws command rows with Codex's terminal-square icon and thin row strokes (s5/f0405, s6/f0526)", () => {
  const { container } = render(
    <ToolActivityRollup
      rollup={{
        kind: "execute",
        commandActivity: "ordinary",
        items: [tool(shell)],
      }}
      interactive
    />,
  );
  const icon = container.querySelector("summary svg[data-activity-icon]");
  expect(icon).toHaveClass("lucide-square-terminal");
  expect(container.querySelector(".lucide-terminal")).toBeNull();
});

it("opens a Web run row to its plaintext result block (s5/f0405)", () => {
  const web = tool({ kind: "webSearch", label: "Web search" });
  web.payload.inputSummary = "synthetic native replay";
  const { container } = render(
    <WorkedTurnTerminal value>
      <ToolActivityRollup
        rollup={{
          kind: "webSearch",
          commandActivity: "ordinary",
          items: [web],
        }}
        interactive
      />
    </WorkedTurnTerminal>,
  );
  const row = container.querySelector("details.acp-tool-rollup")!;
  const summary = row.querySelector("summary")!;
  expect(summary).toHaveTextContent("Web run");
  expect(row.querySelector(".acp-tool-result")).toBeNull();
  expect(summary.querySelector(".acp-tool-rollup__chevron")).not.toBeNull();
  (row as HTMLDetailsElement).open = true;
  fireEvent(row, new Event("toggle"));
  const result = row.querySelector(".acp-tool-result");
  expect(result?.querySelector(".acp-code-block__language")).toHaveTextContent(
    "plaintext",
  );
  expect(result).toHaveTextContent("synthetic native replay");
});

it("draws a right chevron that turns down when the row opens (s5/f0390)", () => {
  const { container } = render(
    <ToolActivityRollup
      rollup={{ kind: "mcp", commandActivity: "ordinary", items: [tool(mcp)] }}
      interactive
    />,
  );
  expect(container.querySelector(".acp-tool-rollup__chevron")).toHaveClass(
    "lucide-chevron-right",
  );
});

const shell: ToolPresentation = {
  kind: "execute",
  label: "Command",
  summary: "curl.exe -s https://example.com",
  cwd: null,
  exitCode: null,
  outputTail: null,
  outputTruncated: false,
  resources: [],
  activity: "ordinary",
};

it.each(["waitingForApproval", "pending"] as const)(
  "names a %s command with the live Running label and shimmer (s6/f0526)",
  (toolState) => {
    const { container } = render(
      <ToolActivityRollup
        rollup={{
          kind: "execute",
          commandActivity: "ordinary",
          items: [tool(shell, toolState)],
        }}
        interactive
      />,
    );
    const label = container.querySelector(".acp-tool-rollup__label");
    expect(label).toHaveTextContent("Running curl.exe -s https://example.com");
    expect(label).toHaveClass("acp-live-shimmer");
  },
);

it("names an unresolved command in an ended turn as Ran, without shimmer", () => {
  const { container } = render(
    <WorkedTurnTerminal value>
      <ToolActivityRollup
        rollup={{
          kind: "execute",
          commandActivity: "ordinary",
          items: [tool(shell, "waitingForApproval")],
        }}
        interactive
      />
    </WorkedTurnTerminal>,
  );
  const label = container.querySelector(".acp-tool-rollup__label");
  expect(label).toHaveTextContent("Ran curl.exe -s https://example.com");
  expect(label).not.toHaveClass("acp-live-shimmer");
});
