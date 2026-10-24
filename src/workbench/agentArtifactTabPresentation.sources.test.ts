import { expect, it } from "vitest";

import type { AcpChatPage, AcpItem, ToolPresentation } from "../domain/acp";
import { projectTaskResources } from "./agentArtifactTabPresentation";

function toolCall(id: string, presentation: ToolPresentation): AcpItem {
  return {
    id,
    kind: "toolCall",
    toolState: "completed",
    payload: { presentation, inputSummary: null },
  } as unknown as AcpItem;
}

function sources(items: AcpItem[]) {
  const page = {
    projectId: "project",
    chatId: "chat",
    turns: [{ id: "turn", sessionId: "session", items, browserOperations: [] }],
  } as unknown as AcpChatPage;
  return projectTaskResources(page, "chat").sources.map(
    (source) => source.label,
  );
}

const mcp = (server: string): ToolPresentation => ({
  kind: "mcp",
  label: "search",
  serverLabel: server,
  toolLabel: "search",
  argumentsSummary: null,
  resultSummary: null,
});

it("lists each integration once by the harness's server name, beside web search", () => {
  expect(
    sources([
      toolCall("one", mcp("openaiDeveloperDocs")),
      toolCall("two", { kind: "webSearch", label: "Web search" }),
      toolCall("three", mcp("openaiDeveloperDocs")),
      toolCall("four", {
        kind: "fetch",
        label: "Fetch",
        url: "https://example.com/a",
        summary: null,
      }),
    ]),
  ).toEqual(["openaiDeveloperDocs", "Search Service", "example.com"]);
});

it("never invents a source for an MCP call whose server the harness did not name", () => {
  expect(sources([toolCall("one", mcp("MCP"))])).toEqual([]);
});
