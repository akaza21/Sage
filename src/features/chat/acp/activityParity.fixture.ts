import type { AcpItem, ToolPresentation } from "../../../domain/acp";

export function parityTool(
  kind: "read" | "edit" | "execute" = "execute",
  index = 1,
  toolState: Extract<AcpItem, { kind: "toolCall" }>["toolState"] = "completed",
): Extract<AcpItem, { kind: "toolCall" }> {
  const presentation: ToolPresentation =
    kind === "execute"
      ? {
          kind,
          label: "git status --short",
          summary: "git status --short",
          cwd: null,
          exitCode: 0,
          outputTail: null,
          outputTruncated: false,
          resources: [],
          activity: "ordinary",
        }
      : {
          kind,
          label: kind,
          resources: [
            {
              kind: "resource",
              resource: {
                rootId: "root-1",
                relativePath: `src/example-${index}.ts`,
              },
            },
          ],
        };
  return {
    id: `tool-${index}`,
    projectId: "project-1",
    chatId: "chat-1",
    sessionId: "session-1",
    turnId: "turn-1",
    runtimeEpoch: "2",
    ordinal: index + 1,
    origin: "primary",
    activityId: null,
    lifecycle: toolState === "completed" ? "completed" : "streaming",
    revision: "1",
    createdAt: 1_000,
    updatedAt: 2_000,
    terminalAt: toolState === "completed" ? 2_000 : null,
    kind: "toolCall",
    toolState,
    payload: {
      presentation,
      inputSummary:
        kind === "execute" ? "git status --short" : `src/example-${index}.ts`,
      outputSummary: "fixture output",
    },
  } as Extract<AcpItem, { kind: "toolCall" }>;
}
