import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import {
  agentArtifactFollowRecordFor,
  useAgentArtifactFollowStore,
} from "../../../app/agentArtifactFollowStore";
import type { AcpItem } from "../../../domain/acp";
import {
  browseSessionForOwner,
  clearBrowseSessions,
  setHarnessBrowseOwner,
  type HarnessBrowseOwner,
} from "../../preview/harnessBrowse";
import type { AgentArtifactOwner } from "../../../domain/agentArtifact";
import { ToolItem } from "./ToolItem";
import { toolCommandLine } from "./toolActivityRollup";

afterEach(() => {
  cleanup();
  clearBrowseSessions();
  useAgentArtifactFollowStore.getState().clear();
});

type ToolProjectionItem = Extract<AcpItem, { kind: "toolCall" }>;
const browseOwner = {
  projectId: "project-1",
  chatId: "chat-1",
  sessionId: "session-1",
} as AgentArtifactOwner;
const previewOwner = {
  ...browseOwner,
  projectGeneration: "1",
  transitionEpoch: 1,
  runtimeEpoch: "1",
} as HarnessBrowseOwner;

beforeEach(() => setHarnessBrowseOwner(previewOwner, [previewOwner.chatId]));

function tool(
  presentation: ToolProjectionItem["payload"]["presentation"],
  toolState: ToolProjectionItem["toolState"] = "inProgress",
  terminalAt: number | null = null,
): ToolProjectionItem {
  return {
    id: "item-1",
    projectId: "project-1",
    chatId: "chat-1",
    sessionId: "session-1",
    turnId: "turn-1",
    runtimeEpoch: "1",
    ordinal: 1,
    origin: "primary",
    activityId: null,
    lifecycle: "streaming",
    revision: "1",
    createdAt: 1,
    updatedAt: 1,
    terminalAt,
    kind: "toolCall",
    toolState,
    payload: { presentation, inputSummary: "Bounded retained input" },
  } as ToolProjectionItem;
}

describe("ToolItem", () => {
  it("opens canonical file and diff ResourceRefs while leaving foreign paths inert", () => {
    const onOpen = vi.fn();
    const { rerender } = render(
      <ToolItem
        item={tool({
          kind: "read",
          label: "Read files",
          resources: [
            {
              kind: "resource",
              resource: { rootId: "root-a", relativePath: "src/main.ts" },
            },
            {
              kind: "unavailable",
              reason: "foreign",
              label: "Outside project",
            },
          ],
        })}
        onOpenResource={onOpen}
      />,
    );
    const fileAction = screen.getByRole("button", {
      name: "Open file src/main.ts",
    });
    expect(fileAction).toHaveAttribute("data-resource-root-id", "root-a");
    expect(fileAction).toHaveAttribute(
      "data-resource-relative-path",
      "src/main.ts",
    );
    fireEvent.click(fileAction);
    expect(onOpen).toHaveBeenCalledWith(
      { rootId: "root-a", relativePath: "src/main.ts" },
      "file",
    );
    expect(
      screen.getByText("Outside project · unavailable"),
    ).toBeInTheDocument();

    rerender(
      <ToolItem
        item={tool({
          kind: "edit",
          label: "Changed file",
          resources: [
            {
              kind: "resource",
              resource: { rootId: "root-b", relativePath: "src/main.ts" },
            },
          ],
        })}
        onOpenResource={onOpen}
      />,
    );
    fireEvent.click(
      screen.getByRole("button", { name: "Open file src/main.ts" }),
    );
    expect(onOpen).toHaveBeenLastCalledWith(
      { rootId: "root-b", relativePath: "src/main.ts" },
      "file",
    );
  });

  function readItemWithOutput(
    outputSummary: string | null,
  ): ToolProjectionItem {
    const item = tool({
      kind: "read",
      label: "Read alpha.txt",
      resources: [
        {
          kind: "resource",
          resource: { rootId: "root-a", relativePath: "alpha.txt" },
        },
      ],
    });
    item.payload.outputSummary = outputSummary;
    return item;
  }

  it("discloses retained read output as bounded escaped text without claiming resources are the result", () => {
    const onOpen = vi.fn();
    const { rerender } = render(
      <ToolItem
        item={readItemWithOutput("1\tread-only harness probe")}
        onOpenResource={onOpen}
      />,
    );

    const output = screen.getByLabelText("Tool output");
    expect(output).toBeVisible();
    expect(output.tagName).toBe("PRE");
    expect(output.textContent).toBe("1\tread-only harness probe");
    expect(screen.getByText("Tool output")).toBeVisible();
    expect(
      screen.getByRole("button", { name: "Open file alpha.txt" }),
    ).toBeVisible();
    expect(screen.queryByText("Tool details")).toBeNull();
    const card = screen.getByRole("group", { name: "Read alpha.txt" });
    expect(
      document.getElementById(card.getAttribute("aria-describedby") ?? ""),
    ).toBe(output);
    expect(card.textContent?.match(/read-only harness probe/g)).toHaveLength(1);

    const markup = "<img src=x onerror=alert(1)>";
    rerender(<ToolItem item={readItemWithOutput(markup)} />);
    const escaped = screen.getByLabelText("Tool output");
    expect(escaped.textContent).toBe(markup);
    expect(escaped.querySelector("img")).toBeNull();

    const oversize = `[output truncated] ${"probe line\n".repeat(200)}`;
    rerender(<ToolItem item={readItemWithOutput(oversize)} />);
    expect(screen.getByLabelText("Tool output").textContent).toBe(oversize);

    rerender(<ToolItem item={readItemWithOutput(null)} />);
    expect(screen.queryByLabelText("Tool output")).toBeNull();
    expect(screen.getByText("Tool details")).toBeInTheDocument();
  });

  it("never labels an admitted browse href as tool output", () => {
    const fetchItem = tool({
      kind: "fetch",
      label: "Fetch page",
      url: null,
      summary: null,
    } as ToolProjectionItem["payload"]["presentation"]);
    fetchItem.payload.inputSummary = null;
    fetchItem.payload.outputSummary = "http://127.0.0.1:5173/app";
    render(<ToolItem item={fetchItem} />);

    expect(screen.queryByLabelText("Tool output")).toBeNull();
    expect(
      screen.getByRole("button", { name: "Open in Sage browser" }),
    ).toBeVisible();
  });

  it("keeps harness truncation text verbatim while identifying retained output as Sage chrome", () => {
    const output =
      "[output truncated] retained tail\n\tindented line\n\nfinal line\n";
    render(<ToolItem item={commandItem(output)} />);

    expect(screen.getByText("Retained command output")).toBeVisible();
    expect(screen.getByLabelText("Command output").textContent).toBe(output);
  });

  it("shows command output and its safe File action in one card", () => {
    const onOpen = vi.fn();
    const fileItem = tool({
      kind: "read",
      label: "Read file",
      resources: [
        {
          kind: "resource",
          resource: { rootId: "root-a", relativePath: "src/main.ts" },
        },
      ],
    });
    fileItem.payload.inputSummary = null;
    const { rerender } = render(
      <ToolItem item={fileItem} onOpenResource={vi.fn()} />,
    );

    expect(
      screen.getByRole("button", { name: "Open file src/main.ts" }),
    ).toBeVisible();
    expect(screen.queryByText("Tool details")).toBeNull();

    const command = {
      ...tool({
        kind: "execute",
        cwd: null,
        exitCode: null,
        outputTail: null,
        outputTruncated: false,
        label: "Run tests",
        summary: null,
        resources: [
          {
            kind: "resource" as const,
            resource: { rootId: "root-a", relativePath: "package.json" },
          },
        ],
        activity: "ordinary",
      }),
      payload: {
        presentation: {
          kind: "execute" as const,
          cwd: null,
          exitCode: null,
          outputTail: null,
          outputTruncated: false,
          label: "Run tests",
          summary: null,
          resources: [
            {
              kind: "resource" as const,
              resource: { rootId: "root-a", relativePath: "package.json" },
            },
          ],
          activity: "ordinary",
        },
        inputSummary: null,
        outputSummary: "retained command output",
      },
    } as Extract<AcpItem, { kind: "toolCall" }>;
    rerender(<ToolItem item={command} onOpenResource={onOpen} />);
    expect(screen.getByLabelText("Command output")).toHaveTextContent(
      "retained command output",
    );
    const commandCard = screen.getByRole("group", { name: "Run tests" });
    const descriptionId = commandCard.getAttribute("aria-describedby");
    expect(descriptionId).not.toBeNull();
    expect(document.getElementById(descriptionId ?? "")).toBe(
      screen.getByLabelText("Command output"),
    );
    expect(screen.getAllByText("retained command output")).toHaveLength(1);
    expect(screen.queryByText("Tool details")).toBeNull();
    fireEvent.click(
      screen.getByRole("button", { name: "Open file package.json" }),
    );
    expect(onOpen).toHaveBeenLastCalledWith(
      { rootId: "root-a", relativePath: "package.json" },
      "file",
    );
  });

  it("uses one referenced output surface for toolCall command output transitions", () => {
    const { rerender } = render(<ToolItem item={commandItem(null)} />);
    expect(
      screen.getByRole("group", { name: "Run command" }),
    ).not.toHaveAttribute("aria-describedby");
    expect(screen.queryByLabelText("Command output")).toBeNull();

    rerender(<ToolItem item={commandItem("bounded output")} />);
    const card = screen.getByRole("group", { name: "Run command" });
    const id = card.getAttribute("aria-describedby");
    expect(document.getElementById(id ?? "")).toBe(
      screen.getByLabelText("Command output"),
    );
    expect(screen.getAllByText("bounded output")).toHaveLength(1);
    expect(screen.queryByText("Tool details")).toBeNull();

    rerender(<ToolItem item={commandItem("")} />);
    expect(
      screen.getByRole("group", { name: "Run command" }),
    ).not.toHaveAttribute("aria-describedby");
    expect(screen.queryByLabelText("Command output")).toBeNull();
  });

  it("adopts a terminal state and duration on one card when a call updates in place", () => {
    const presentation = {
      kind: "execute" as const,
      cwd: null,
      exitCode: null,
      outputTail: null,
      outputTruncated: false,
      label: "Run migration",
      summary: null,
      resources: [],
      activity: "ordinary" as const,
    };
    const { rerender, container } = render(
      <ToolItem item={tool(presentation, "inProgress", null)} />,
    );
    expect(container.querySelector(".acp-tool-card")).toHaveAttribute(
      "data-activity-state",
      "inProgress",
    );
    expect(container.querySelector(".acp-tool-card__duration")).toBeNull();

    rerender(
      <ToolItem
        item={{
          ...tool(presentation, "completed", 4_000),
          createdAt: 1_000,
        }}
      />,
    );

    expect(container.querySelectorAll(".acp-tool-card")).toHaveLength(1);
    const card = container.querySelector(".acp-tool-card");
    expect(card).toHaveAttribute("data-activity-state", "completed");
    expect(card).toHaveAttribute("data-acp-item-id", "item-1");
    expect(
      container.querySelector(".acp-tool-card__duration"),
    ).toHaveTextContent("3s");
  });

  it("omits the command line when it only repeats the tool label", () => {
    const echoed = tool({
      kind: "execute",
      cwd: null,
      exitCode: null,
      outputTail: null,
      outputTruncated: false,
      label: "npm run check",
      summary: "npm run check",
      resources: [],
      activity: "ordinary",
    });
    echoed.payload.inputSummary = null;
    const { rerender } = render(<ToolItem item={echoed} />);
    expect(screen.queryByLabelText("Command line")).toBeNull();
    expect(screen.getAllByText("npm run check")).toHaveLength(1);

    const distinct = tool({
      kind: "execute",
      cwd: null,
      exitCode: null,
      outputTail: null,
      outputTruncated: false,
      label: "npm run check",
      summary: "npm run check -- --watch",
      resources: [],
      activity: "ordinary",
    });
    distinct.payload.inputSummary = null;
    rerender(<ToolItem item={distinct} />);
    expect(screen.getByLabelText("Command line")).toHaveTextContent(
      "npm run check -- --watch",
    );
  });

  function commandItem(
    outputSummary: string | null,
  ): Extract<AcpItem, { kind: "toolCall" }> {
    const presentation = {
      kind: "execute" as const,
      cwd: null,
      exitCode: null,
      outputTail: null,
      outputTruncated: false,
      label: "Run command",
      summary: null,
      resources: [],
      activity: "ordinary" as const,
    };
    return {
      ...tool(presentation),
      payload: {
        presentation,
        inputSummary: "retained input",
        outputSummary,
      },
    };
  }

  it("preserves the full bounded retained heredoc when the native command summary is null", () => {
    const item = commandItem(null);
    const prefix = "cat <<'EOF'\n";
    const suffix = "\nEOF";
    item.payload.inputSummary =
      prefix + "x".repeat(65_536 - prefix.length - suffix.length) + suffix;
    render(<ToolItem item={item} />);
    const line = screen.getByLabelText("Command line");
    expect(line.textContent).toBe(item.payload.inputSummary);
    expect(line.textContent).toHaveLength(65_536);
    expect(screen.queryByLabelText("Command output")).toBeNull();
  });

  it("shows full retained multiline input even when a shorter presentation summary exists", () => {
    const item = commandItem(null);
    if (item.payload.presentation.kind !== "execute")
      throw new Error("Expected execute presentation");
    item.payload.inputSummary = "cat <<'EOF'\nfull retained body\nEOF";
    item.payload.presentation.summary = "cat <<'EOF'";
    render(<ToolItem item={item} />);
    expect(screen.getByLabelText("Command line").textContent).toBe(
      "cat <<'EOF'\nfull retained body\nEOF",
    );
  });

  it.each(["`npm run check`", "  ` npm run check `  ", "``npm run check``"])(
    "does not repeat retained command input under a backticked title %s",
    (label) => {
      const item = commandItem(null);
      item.payload.presentation.label = label;
      item.payload.inputSummary = "npm run check";
      render(<ToolItem item={item} />);
      expect(screen.getByRole("group")).toBeVisible();
      expect(document.querySelector(".acp-tool-card strong")?.textContent).toBe(
        label,
      );
      expect(screen.queryByLabelText("Command line")).toBeNull();
    },
  );

  it("keeps the multiline command pre even when its title repeats the retained input", () => {
    const item = commandItem(null);
    item.payload.inputSummary = "cat <<'EOF'\nbody\nEOF";
    item.payload.presentation.label = item.payload.inputSummary;
    render(<ToolItem item={item} />);
    expect(screen.getByLabelText("Command line").textContent).toBe(
      "cat <<'EOF'\nbody\nEOF",
    );
  });

  it("prefers a nonblank native summary and otherwise uses retained command input", () => {
    const item = commandItem(null);
    if (item.payload.presentation.kind !== "execute")
      throw new Error("Expected execute presentation");
    item.payload.inputSummary = "npm run check";
    item.payload.presentation.summary = "npm run lint";
    expect(toolCommandLine(item)).toBe("npm run lint");
    item.payload.presentation.summary = " \t ";
    expect(toolCommandLine(item)).toBe("npm run check");
    item.payload.inputSummary = "Run command";
    expect(toolCommandLine(item)).toBe("Run command");
    item.payload.inputSummary = null;
    expect(toolCommandLine(item)).toBeNull();
  });

  it("normalizes command text to one line and truncates without splitting a surrogate pair", () => {
    const item = commandItem(null);
    item.payload.inputSummary = " npm\nrun\tcheck ";
    expect(toolCommandLine(item)).toBe("npm run check");
    item.payload.inputSummary = "x".repeat(510) + "\u{1f680}".repeat(1_000);
    expect(toolCommandLine(item)).toBe("x".repeat(510) + "\u2026");
    expect(toolCommandLine(item)!.length).toBeLessThanOrEqual(512);
  });

  it("does not invent command text for non-command presentations", () => {
    expect(
      toolCommandLine(
        tool({
          kind: "other",
          label: "Unknown tool",
          summary: null,
          rawKind: null,
        }),
      ),
    ).toBeNull();
  });

  it("preserves literal quoted spaces in the expanded retained command", () => {
    const item = commandItem(null);
    item.payload.inputSummary = "printf 'two  spaces'";
    expect(toolCommandLine(item)).toBe("printf 'two  spaces'");
    render(<ToolItem item={item} />);
    expect(screen.getByLabelText("Command line").textContent).toBe(
      "printf 'two  spaces'",
    );
  });

  it("uses one generic card for unknown tools and typed safe summaries", () => {
    const { rerender } = render(
      <ToolItem
        item={tool({
          kind: "other",
          rawKind: null,
          label: "Unknown provider tool",
          summary: "Normalized fallback",
        })}
      />,
    );
    expect(
      screen.getByRole("group", {
        name: "Unknown provider tool",
        description: "Bounded retained input",
      }),
    ).toHaveTextContent("Normalized fallback");
    expect(screen.getByRole("status")).toHaveTextContent("In progress");

    rerender(
      <ToolItem
        item={tool({
          kind: "mcp",
          argumentsSummary: null,
          resultSummary: null,
          label: "MCP tool",
          serverLabel: "Issue tracker",
          toolLabel: "Read issue",
        })}
      />,
    );
    expect(screen.getByText("Issue tracker · Read issue")).toBeInTheDocument();
  });

  it("puts an MCP identity on its activity line without repeating generic detail", () => {
    const { container } = render(
      <ToolItem
        item={tool({
          kind: "mcp",
          argumentsSummary: null,
          resultSummary: null,
          label: "MCP tool",
          serverLabel: "Issue tracker",
          toolLabel: "Read issue",
        })}
      />,
    );
    const row = container.querySelector(".acp-tool-card__header");
    expect(row).toHaveTextContent("Issue tracker · Read issue");
    expect(container.querySelectorAll(".acp-tool-card p")).toHaveLength(0);
    expect(container.querySelectorAll(".acp-details")).toHaveLength(0);
  });

  it("keeps a terminal tool state accessible without replaying it", () => {
    render(
      <ToolItem
        item={tool(
          {
            kind: "other",
            rawKind: null,
            label: "Unknown provider tool",
            summary: "Normalized fallback",
          },
          "completed",
        )}
      />,
    );
    const status = screen.getByRole("status", {
      name: "Unknown provider tool: Completed",
    });
    expect(status).toHaveTextContent("Completed");
    expect(status).toHaveAttribute("aria-live", "off");
    expect(status).toHaveAttribute("aria-atomic", "true");
  });

  it.each([
    "pending",
    "inProgress",
    "waitingForApproval",
    "completed",
    "failed",
    "declined",
    "cancelled",
  ] as const)("exposes the %s state to the shared card grammar", (state) => {
    const { container } = render(
      <ToolItem
        item={tool(
          {
            kind: "other",
            rawKind: null,
            label: "Tool state",
            summary: "Bounded detail",
          },
          state,
        )}
      />,
    );
    expect(container.querySelector(".acp-activity-card--tool")).toHaveAttribute(
      "data-activity-kind",
      "other",
    );
    expect(container.querySelector(".acp-activity-card--tool")).toHaveAttribute(
      "data-activity-state",
      state,
    );
  });

  it("renders the kind icon, title, status and duration on one row", () => {
    const { container } = render(
      <ToolItem
        item={tool(
          {
            kind: "execute",
            label: "Run command",
            summary: null,
            cwd: null,
            exitCode: 0,
            outputTail: null,
            outputTruncated: false,
            resources: [],
            activity: "ordinary",
          },
          "completed",
          4_500,
        )}
      />,
    );
    const header = container.querySelector(".acp-tool-card__header");
    expect(
      header?.querySelector('[data-tool-kind-icon="execute"]'),
    ).not.toBeNull();
    expect(header).toHaveTextContent("Run command");
    expect(header).toHaveTextContent("Completed");
    expect(header?.querySelector(".acp-tool-card__duration")).toHaveTextContent(
      "4s",
    );
  });

  it("labels a sub-second and a multi-minute tool duration without lying", () => {
    const { container, rerender } = render(
      <ToolItem
        item={tool(
          { kind: "other", rawKind: null, label: "Quick", summary: null },
          "completed",
          400,
        )}
      />,
    );
    expect(
      container.querySelector(".acp-tool-card__duration"),
    ).toHaveTextContent("<1s");

    rerender(
      <ToolItem
        item={tool(
          { kind: "other", rawKind: null, label: "Slow", summary: null },
          "completed",
          125_001,
        )}
      />,
    );
    expect(
      container.querySelector(".acp-tool-card__duration"),
    ).toHaveTextContent("2m 5s");
  });

  it("names the unknown provider kind on the other fallback row", () => {
    const { container, rerender } = render(
      <ToolItem
        item={tool({
          kind: "other",
          rawKind: "teleport",
          label: "Teleported",
          summary: "raw input preserved",
        })}
      />,
    );
    expect(
      container.querySelector(".acp-tool-card__raw-kind"),
    ).toHaveTextContent("teleport");

    rerender(
      <ToolItem
        item={tool({
          kind: "other",
          rawKind: null,
          label: "Plain",
          summary: null,
        })}
      />,
    );
    expect(container.querySelector(".acp-tool-card__raw-kind")).toBeNull();
  });

  it("omits the duration until the tool reaches a terminal state", () => {
    const { container } = render(
      <ToolItem
        item={tool({
          kind: "read",
          label: "Read files",
          resources: [],
        })}
      />,
    );
    expect(container.querySelector(".acp-tool-card__duration")).toBeNull();
    expect(
      container.querySelector('[data-tool-kind-icon="read"]'),
    ).not.toBeNull();
  });

  it("opens an admitted loopback web tool in the Sage browser", () => {
    render(
      <ToolItem
        item={tool({
          kind: "fetch",
          url: null,
          label: "Browse preview",
          summary: "Opened http://127.0.0.1:5173/app",
        })}
      />,
    );
    fireEvent.click(
      screen.getByRole("button", { name: "Open in Sage browser" }),
    );
    expect(browseSessionForOwner(previewOwner)?.href).toBe(
      "http://127.0.0.1:5173/app",
    );
    expect(
      agentArtifactFollowRecordFor(
        useAgentArtifactFollowStore.getState().records,
        browseOwner,
      ),
    ).toMatchObject({ mode: "pinned", selected: { kind: "preview" } });
  });

  it("does not pin a historical or non-interactive browse item", () => {
    render(
      <ToolItem
        allowBrowse={false}
        item={tool({
          kind: "fetch",
          url: null,
          label: "Browse preview",
          summary: "Opened http://127.0.0.1:5173/history",
        })}
      />,
    );
    expect(
      screen.queryByRole("button", { name: "Open in Sage browser" }),
    ).toBeNull();
    expect(browseSessionForOwner(previewOwner)).toBeNull();
    expect(useAgentArtifactFollowStore.getState().records).toEqual([]);
  });

  it("keeps canonical resources inert until a native open handler is wired", () => {
    render(
      <ToolItem
        item={tool({
          kind: "read",
          label: "Read file",
          resources: [
            {
              kind: "resource",
              resource: { rootId: "root-a", relativePath: "src/main.ts" },
            },
          ],
        })}
      />,
    );
    expect(screen.queryByRole("button", { name: /open file/i })).toBeNull();
    expect(screen.getByText("main.ts")).toBeInTheDocument();
    expect(
      document.querySelector(".file-kind-icon--typescript"),
    ).not.toBeNull();
  });
});
