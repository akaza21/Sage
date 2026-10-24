import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";

import type { AcpItem, ToolPresentation } from "../../../domain/acp";
import { CompactToolItem } from "./CompactToolItem";
import { ToolItem } from "./ToolItem";

afterEach(cleanup);

type ToolProjectionItem = Extract<AcpItem, { kind: "toolCall" }>;

function tool(
  presentation: ToolPresentation,
  toolState: ToolProjectionItem["toolState"] = "completed",
  outputSummary: string | null = null,
  inputSummary: string | null = null,
): ToolProjectionItem {
  return {
    id: "tool-1",
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
    payload: { presentation, inputSummary, outputSummary },
  } as ToolProjectionItem;
}

const mcp: ToolPresentation = {
  kind: "mcp",
  label: "search_openai_docs",
  serverLabel: "openaiDeveloperDocs",
  toolLabel: "search_openai_docs",
  argumentsSummary: null,
  resultSummary: null,
};

function execute(exitCode: number | null): ToolPresentation {
  return {
    kind: "execute",
    label: "npm test",
    summary: "npm test",
    cwd: null,
    exitCode,
    outputTail: null,
    outputTruncated: false,
    resources: [],
    activity: "ordinary",
  };
}

describe("integration results", () => {
  it("shows a JSON MCP result as a json code block inside a bounded result", () => {
    const { container } = render(
      <ToolItem
        compact
        item={tool(
          mcp,
          "completed",
          '{\n  "hits": [\n    {"url": "a"}\n  ]\n}',
        )}
      />,
    );
    const result = container.querySelector(".acp-tool-result");
    expect(result?.querySelector(".acp-code-block")).not.toBeNull();
    expect(
      result?.querySelector(".acp-code-block__language"),
    ).toHaveTextContent("JSON");
    expect(result).toHaveTextContent('"hits"');
    expect(container.querySelector(".acp-tool-command-frame")).toBeNull();
  });

  it("shows non-JSON or clipped output as plain text", () => {
    const { container } = render(
      <ToolItem
        compact
        item={tool(mcp, "completed", '{"hits": [\n… truncated')}
      />,
    );
    expect(
      container.querySelector(".acp-code-block__language"),
    ).toHaveTextContent("plaintext");
  });

  it("shows only the harness's message in an alert box when the call failed", () => {
    render(<ToolItem compact item={tool(mcp, "failed", "sandbox probe")} />);
    const alert = screen.getByRole("alert");
    expect(alert).toHaveClass("acp-tool-error");
    expect(alert.textContent).toBe("sandbox probe");
    expect(screen.queryByText("Error")).toBeNull();
  });

  it("shows nothing when the harness carried no result", () => {
    const { container } = render(<ToolItem compact item={tool(mcp)} />);
    expect(container.querySelector(".acp-tool-result")).toBeNull();
    expect(screen.queryByRole("alert")).toBeNull();
  });

  it("shows a web search query when the wire carried one, then its results", () => {
    const { container } = render(
      <ToolItem
        compact
        item={tool(
          { kind: "webSearch", label: "Web search" },
          "completed",
          "Node.js node:test documentation",
          "Node.js node:test documentation",
        )}
      />,
    );
    expect(container.querySelector("p")).toHaveTextContent(
      "Node.js node:test documentation",
    );
    expect(
      container.querySelector(".acp-code-block__language"),
    ).toHaveTextContent("plaintext");
  });
});

describe("Shell outcome", () => {
  it.each([
    [0, "completed", "✓ Success", false],
    [null, "completed", "✓ Success", false],
    [2, "failed", "✕ Exit code 2", true],
    [null, "failed", "Failed", false],
  ] as const)(
    "reads exit %s in state %s as %s",
    (exitCode, state, outcome, danger) => {
      const { container } = render(
        <CompactToolItem
          item={tool(execute(exitCode), state, "ok", "npm test")}
        />,
      );
      const status = container.querySelector(".acp-tool-command__outcome");
      expect(status).toHaveTextContent(outcome);
      expect(
        status?.classList.contains("acp-tool-command__outcome--danger"),
      ).toBe(danger);
    },
  );
});
