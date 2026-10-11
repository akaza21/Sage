import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";

import type { AcpItem } from "../../../domain/acp";
import { ToolItem } from "./ToolItem";
import createdFileStandardAcp from "./createdFileStandardAcp.fixture.json";

afterEach(cleanup);

type ToolProjectionItem = Extract<AcpItem, { kind: "toolCall" }>;
type ToolPresentation = ToolProjectionItem["payload"]["presentation"];

const resource = {
  kind: "resource" as const,
  resource: { rootId: "root-a", relativePath: "src/main.ts" },
};

function item(
  presentation: ToolPresentation,
  toolState: ToolProjectionItem["toolState"] = "completed",
  inputSummary: string | null = null,
  outputSummary: string | null = null,
): ToolProjectionItem {
  return {
    id: "matrix-item",
    projectId: "project-1",
    chatId: "chat-1",
    sessionId: "session-1",
    turnId: "turn-1",
    runtimeEpoch: "1",
    ordinal: 1,
    origin: "primary",
    activityId: null,
    lifecycle: toolState === "inProgress" ? "streaming" : "completed",
    revision: "1",
    createdAt: 1_000,
    updatedAt: 2_000,
    terminalAt: toolState === "inProgress" ? null : 4_000,
    kind: "toolCall",
    toolState,
    payload: { presentation, inputSummary, outputSummary },
  } as ToolProjectionItem;
}

const presentations: readonly [string, ToolPresentation][] = [
  ["read", { kind: "read", label: "Read src/main.ts", resources: [resource] }],
  ["edit", { kind: "edit", label: "Edit [redacted]", resources: [resource] }],
  [
    "delete",
    { kind: "delete", label: "Delete src/stale.ts", resources: [resource] },
  ],
  [
    "move",
    {
      kind: "move",
      label: "Move src/old.ts",
      resources: [resource],
      originalResources: [resource],
    },
  ],
  [
    "search",
    {
      kind: "search",
      label: "Search workspace",
      querySummary: "toolState",
      resources: [resource],
      matchCount: 14,
      truncated: true,
    },
  ],
  [
    "execute",
    {
      kind: "execute",
      label: "Run checks",
      summary: "npm run check",
      cwd: resource,
      exitCode: 1,
      outputTail: "1 failing",
      outputTruncated: true,
      resources: [resource],
      activity: "ordinary",
    },
  ],
  [
    "fetch",
    {
      kind: "fetch",
      label: "Fetch changelog",
      url: "https://example.invalid/changelog",
      summary: "Read the release notes",
    },
  ],
  ["think", { kind: "think", label: "Thinking", summary: "Weighing options" }],
  [
    "switchMode",
    {
      kind: "switchMode",
      label: "Switch mode",
      modeId: "plan",
      modeLabel: "Plan mode",
    },
  ],
  [
    "mcp",
    {
      kind: "mcp",
      label: "Call MCP tool",
      serverLabel: "sage-server",
      toolLabel: "lookup",
      argumentsSummary: "{ id: 4 }",
      resultSummary: "one record",
    },
  ],
  [
    "subagent",
    { kind: "subagent", label: "Delegate to subagent", activityId: null },
  ],
  [
    "other",
    {
      kind: "other",
      label: "quantum_teleport",
      rawKind: "quantum_teleport",
      summary: "Unrecognised harness tool",
    },
  ],
];

const PROVENANCE: Readonly<
  Record<string, "captured" | "contract" | "synthetic">
> = {
  read: "captured",
  switchMode: "contract",
  other: "contract",
  edit: "contract",
  execute: "contract",
  delete: "synthetic",
  move: "synthetic",
  search: "synthetic",
  fetch: "synthetic",
  think: "synthetic",
  mcp: "synthetic",
  subagent: "synthetic",
};

describe("tool render matrix", () => {
  it("declares the evidence tier behind every kind in the matrix", () => {
    expect(Object.keys(PROVENANCE).sort()).toEqual(
      presentations.map(([kind]) => kind).sort(),
    );
    expect(PROVENANCE.read).toBe("captured");
  });

  it.each(presentations)(
    "renders the %s kind with an icon, title and status",
    (kind, presentation) => {
      const { container } = render(<ToolItem item={item(presentation)} />);
      const card = container.querySelector(".acp-tool-card");

      expect(card).toHaveAttribute("data-activity-kind", kind);
      expect(
        card?.querySelector(`[data-tool-kind-icon="${kind}"]`),
      ).not.toBeNull();
      if (presentation.kind === "mcp") {
        const identity = `${presentation.serverLabel} · ${presentation.toolLabel}`;
        expect(screen.getAllByText(identity)).toHaveLength(1);
        expect(card).toHaveAttribute("aria-label", identity);
        expect(screen.queryByText(presentation.label)).toBeNull();
        expect(card?.querySelector(".acp-details")).toBeNull();
      } else {
        expect(screen.getAllByText(presentation.label)).toHaveLength(1);
      }
      expect(card?.querySelector(".acp-state")).toHaveTextContent("Completed");
      expect(card?.querySelector(".acp-tool-card__duration")).toHaveTextContent(
        "3s",
      );
    },
  );

  it.each(presentations)(
    "renders the %s kind while running",
    (_, presentation) => {
      const { container } = render(
        <ToolItem item={item(presentation, "inProgress")} />,
      );
      const card = container.querySelector(".acp-tool-card");

      expect(card).toHaveAttribute("data-activity-state", "inProgress");
      expect(screen.getByRole("status", { name: "Running" })).toBeVisible();
      expect(card?.querySelector(".acp-tool-card__duration")).toBeNull();
    },
  );

  it.each(presentations)(
    "renders the %s kind when it fails",
    (_, presentation) => {
      const { container } = render(
        <ToolItem item={item(presentation, "failed")} />,
      );
      const card = container.querySelector(".acp-tool-card");

      expect(card).toHaveAttribute("data-activity-state", "failed");
      expect(card?.querySelector(".acp-state")).toHaveTextContent("Failed");
    },
  );

  it("keeps a raw unknown kind readable in the other fallback", () => {
    render(
      <ToolItem
        item={item({
          kind: "other",
          label: "harness.raw_tool",
          rawKind: "harness.raw_tool",
          summary: null,
        })}
      />,
    );

    expect(screen.getAllByText("harness.raw_tool")).toHaveLength(1);
    expect(document.querySelector(".acp-tool-card__raw-kind")).toBeNull();
  });

  it("describes a completed new-file edit without a missing-path error", () => {
    render(
      <ToolItem
        item={item({
          kind: "edit",
          label: "Created src/new.ts",
          resources: [
            {
              kind: "unavailable",
              reason: "missing",
              label: "Path does not exist",
            },
          ],
        })}
      />,
    );

    expect(
      screen.getByRole("group", { name: "Created src/new.ts" }),
    ).toBeVisible();
    expect(screen.queryByText(/Path does not exist/i)).toBeNull();
    expect(screen.getByText("File location unavailable")).toBeVisible();
  });

  it("renders the Standard ACP created-file projection from its raw-event fixture", () => {
    render(
      <ToolItem
        item={item({
          kind: "edit",
          label: createdFileStandardAcp.expected.label,
          resources: [
            { kind: "resource", resource: createdFileStandardAcp.resource },
          ],
        })}
      />,
    );

    const card = screen.getByRole("group", {
      name: createdFileStandardAcp.expected.label,
    });
    expect(card).toHaveTextContent("Created src/new.ts");
    expect(card).toHaveTextContent("new.ts");
    expect(card).not.toHaveTextContent("Path does not exist");
  });

  it("keeps a raw kind that differs from the degraded label", () => {
    render(
      <ToolItem
        item={item({
          kind: "other",
          label: "Unsupported activity",
          rawKind: "harness.raw_tool",
          summary: null,
        })}
      />,
    );

    expect(
      document.querySelector(".acp-tool-card__raw-kind"),
    ).toHaveTextContent("harness.raw_tool");
  });

  it("renders the captured cursor-agent read frame with its adversarial id", () => {
    const captured = {
      ...item({ kind: "read", label: "Read File", resources: [resource] }),
      id: "tool-call\nwith-newline" as AcpItem["id"],
    };
    const { container } = render(<ToolItem item={captured} />);
    const card = container.querySelector(".acp-tool-card");

    expect(card).toHaveAttribute("data-acp-item-id", "tool-call\nwith-newline");
    expect(screen.getAllByText("Read File")).toHaveLength(1);
  });

  it("renders an execute whose output was routed through a terminal", () => {
    render(
      <ToolItem
        item={item({
          kind: "execute",
          label: "Run terminal command",
          summary: "pytest -q",
          cwd: null,
          exitCode: null,
          outputTail: null,
          outputTruncated: false,
          resources: [],
          activity: "ordinary",
        })}
      />,
    );

    expect(screen.getByLabelText("Command line")).toHaveTextContent(
      "pytest -q",
    );
    expect(screen.queryByLabelText("Command output")).toBeNull();
    expect(screen.queryByText("Retained command output")).toBeNull();
  });

  it("renders the codex review-mode transition as a switch mode card", () => {
    render(
      <ToolItem
        item={item(
          {
            kind: "switchMode",
            label: "Entered review mode",
            modeId: null,
            modeLabel: null,
          },
          "completed",
          "review",
        )}
      />,
    );

    expect(screen.getAllByText("Entered review mode")).toHaveLength(1);
    expect(screen.getByText("Tool details")).toBeVisible();
    const [accessibleDescription, visibleDetail] =
      screen.getAllByText("review");
    expect(accessibleDescription).toHaveClass("sr-only");
    expect(visibleDetail.closest("details")).not.toBeNull();
  });

  it("renders an untitled unknown item with its raw type and bounded input", () => {
    const rawJson = JSON.stringify({ type: "harness.future_thing" }).repeat(
      400,
    );
    const { container } = render(
      <ToolItem
        item={item(
          {
            kind: "other",
            label: "harness.future_thing",
            rawKind: "harness.future_thing",
            summary: null,
          },
          "completed",
          rawJson,
        )}
      />,
    );

    expect(screen.getAllByText("harness.future_thing")).toHaveLength(1);
    expect(container.querySelector(".acp-tool-card__raw-kind")).toBeNull();
    expect(container.querySelector(".acp-details")).not.toBeNull();
  });

  it.each([
    ["waitingForApproval", "Waiting for approval", null],
    ["declined", "Declined", "Permission declined"],
    ["cancelled", "Cancelled", "Permission cancelled"],
  ] as const)(
    "renders a permission-gated execute in the %s state",
    (toolState, label, outputSummary) => {
      const { container } = render(
        <ToolItem
          item={item(
            {
              kind: "execute",
              label: "Run deploy script",
              summary: "./deploy.sh",
              cwd: null,
              exitCode: null,
              outputTail: null,
              outputTruncated: false,
              resources: [],
              activity: "ordinary",
            },
            toolState,
            null,
            outputSummary,
          )}
        />,
      );

      expect(container.querySelector(".acp-state")).toHaveTextContent(label);
      if (outputSummary)
        expect(screen.getByLabelText("Command output")).toHaveTextContent(
          outputSummary,
        );
    },
  );

  it("shows a retained command tail with its exit code state", () => {
    render(
      <ToolItem
        item={item(
          {
            kind: "execute",
            label: "Run checks",
            summary: "npm run check",
            cwd: resource,
            exitCode: 1,
            outputTail: "1 failing",
            outputTruncated: true,
            resources: [],
            activity: "ordinary",
          },
          "failed",
          null,
          "1 failing",
        )}
      />,
    );

    expect(screen.getByLabelText("Command line")).toHaveTextContent(
      "npm run check",
    );
    expect(screen.getByLabelText("Command output")).toHaveTextContent(
      "1 failing",
    );
  });
});
