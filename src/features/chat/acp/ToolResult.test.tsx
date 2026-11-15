import { readFileSync } from "node:fs";
import { resolve } from "node:path";

import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";

import type { AcpItem, ToolPresentation } from "../../../domain/acp";

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
    expect(
      readFileSync(
        resolve("src/features/chat/acp/acp-activity-details.css"),
        "utf8",
      ),
    ).toMatch(
      /\.acp-tool-result \.acp-code-block__language\s*\{\s*text-transform:\s*lowercase;/,
    );
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

  it("shows a web result only as a plaintext code block, with no query line (Codex s5/f0405)", () => {
    const { container } = render(
      <ToolItem
        compact
        item={tool(
          { kind: "webSearch", label: "Web search" },
          "completed",
          '{"url": "https://nodejs.org/api/test.html"}',
          "Node.js node:test documentation",
        )}
      />,
    );
    expect(container.querySelector("p")).toBeNull();
    expect(screen.queryByText("Node.js node:test documentation")).toBeNull();
    const result = container.querySelector(".acp-tool-result");
    expect(
      result?.querySelector(".acp-code-block__language"),
    ).toHaveTextContent("plaintext");
    expect(result).toHaveTextContent("https://nodejs.org/api/test.html");
    expect(result?.querySelector("pre")).toHaveStyle({
      whiteSpace: "pre-wrap",
    });
    const wrap = screen.getByRole("button", { name: "Wrap code" });
    expect(wrap).toHaveAttribute("aria-pressed", "true");
    fireEvent.click(wrap);
    expect(result?.querySelector("pre")).toHaveStyle({ whiteSpace: "pre" });
  });

  it("shows a web search's query in the plaintext block when no result came back", () => {
    const { container } = render(
      <ToolItem
        compact
        item={tool(
          { kind: "webSearch", label: "Web search" },
          "completed",
          null,
          "Node.js node:test documentation",
        )}
      />,
    );
    expect(container.querySelector("p")).toBeNull();
    const result = container.querySelector(".acp-tool-result");
    expect(
      result?.querySelector(".acp-code-block__language"),
    ).toHaveTextContent("plaintext");
    expect(result).toHaveTextContent("Node.js node:test documentation");
  });

  it("never shows a failed web search's query as an error alert", () => {
    const { container } = render(
      <ToolItem
        compact
        item={tool(
          { kind: "webSearch", label: "Web search" },
          "failed",
          null,
          "Node.js node:test documentation",
        )}
      />,
    );
    expect(screen.queryByRole("alert")).toBeNull();
    expect(container.querySelector(".acp-tool-error")).toBeNull();
    expect(screen.queryByText("Node.js node:test documentation")).toBeNull();
  });

  it("still shows a failed web search's harness message as an alert", () => {
    render(
      <ToolItem
        compact
        item={tool(
          { kind: "webSearch", label: "Web search" },
          "failed",
          "search backend unavailable",
          "Node.js node:test documentation",
        )}
      />,
    );
    expect(screen.getByRole("alert").textContent).toBe(
      "search backend unavailable",
    );
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
        <ToolItem
          compact
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
