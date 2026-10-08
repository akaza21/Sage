import type { AcpItem } from "../domain/acp";
import { truncateUtf8 } from "../domain/text";
import { isRecord, safeIdentifier } from "./acpDesktopResponses";
import { safeItem } from "./acpDesktopResponseItems";
import { invokeNative } from "../ports/nativeInvoke";

export interface ChildTranscriptRequest {
  projectId: string;
  projectGeneration: string;
  chatId: string;
  expectedProjectionRevision: string;
  sessionId: string;
  runtimeEpoch: string;
  itemId: string;
  agentKey: string;
}

export type ChildTranscript =
  | {
      kind: "current" | "fromEarlierSession";
      retentionLimited: boolean;
      turns: { id: string; status: string; items: AcpItem[] }[];
    }
  | { kind: "tooLarge" };

function itemForView(
  value: unknown,
  turnId: string,
  index: number,
  request: ChildTranscriptRequest,
): AcpItem | null {
  if (!isRecord(value) || !isRecord(value.payload)) return null;
  const source = value.payload;
  if (!isRecord(source.payload)) return null;
  const payload = source.payload;
  const base = {
    id: value.id,
    projectId: request.projectId,
    chatId: request.chatId,
    sessionId: request.sessionId,
    turnId,
    runtimeEpoch: request.runtimeEpoch,
    ordinal: index,
    origin: "primary",
    activityId: null,
    lifecycle: value.lifecycle,
    revision: "0",
    createdAt: 0,
    updatedAt: 0,
    terminalAt: null,
  };
  switch (source.kind) {
    case "text":
      return safeItem({
        ...base,
        kind: "text",
        toolState: null,
        payload: { role: value.role ?? "system", text: payload.text },
      });
    case "thoughtSummary":
      return safeItem({
        ...base,
        kind: "thoughtSummary",
        toolState: null,
        payload: { summary: payload.summary },
      });
    case "compactionNotice":
      return safeItem({
        ...base,
        kind: "compactionNotice",
        toolState: null,
        payload,
      });
    case "plan":
      return typeof payload.summary === "string"
        ? safeItem({
            ...base,
            kind: "plan",
            toolState: null,
            payload: {
              entries: [
                {
                  label: truncateUtf8(payload.summary, 512),
                  state:
                    value.lifecycle === "started" ||
                    value.lifecycle === "streaming"
                      ? "inProgress"
                      : "completed",
                },
              ],
            },
          })
        : null;
    case "toolCall": {
      const presentation = childToolPresentation(payload);
      return safeItem({
        ...base,
        kind: "toolCall",
        toolState: value.tool_state,
        payload: {
          presentation,
          inputSummary: payload.inputSummary,
          outputSummary: payload.outputSummary,
        },
      });
    }
    case "fileChangeDiff": {
      const available = payload.pairAvailable === true;
      const truncated = payload.truncated === true;
      return safeItem({
        ...base,
        kind: "fileChangeDiff",
        toolState: null,
        payload: {
          changes: [
            {
              resource: payload.resource,
              originalResource: null,
              operation: payload.operation ?? "updated",
              summary: payload.summary,
              additions: payload.additions ?? null,
              deletions: payload.deletions ?? null,
              pairState: available
                ? "available"
                : truncated
                  ? "summaryTruncated"
                  : "unavailable",
              snapshotRevision: available
                ? String(payload.snapshotRevision)
                : "0",
              oldBytes: available ? payload.oldBytes : null,
              newBytes: available ? payload.newBytes : null,
              contentDigest: available ? payload.contentDigest : null,
            },
          ],
          truncated,
        },
      });
    }
    case "unsupported":
      return safeItem({
        ...base,
        kind: "unsupported",
        toolState: null,
        payload: {
          category: payload.externalKind,
          summary: payload.summary,
          truncated: false,
        },
      });
    default:
      return safeItem({
        ...base,
        kind: "unsupported",
        toolState: null,
        payload: {
          category: "childActivity",
          summary: "Child activity is not displayed in this view.",
          truncated: false,
        },
      });
  }
}

function childToolPresentation(payload: Record<string, unknown>): unknown {
  const label = payload.label;
  const resources = payload.locations;
  switch (payload.category) {
    case "read":
    case "edit":
    case "delete":
      return { kind: payload.category, label, resources };
    case "move":
      return { kind: "move", label, resources, originalResources: [] };
    case "search":
      return {
        kind: "search",
        label,
        querySummary: label,
        resources: [],
        matchCount: null,
        truncated: false,
      };
    case "execute": {
      const execution = isRecord(payload.execution) ? payload.execution : {};
      return {
        kind: "execute",
        label,
        resources,
        summary: null,
        cwd: execution.cwd ?? null,
        exitCode: execution.exitCode ?? null,
        outputTail: execution.outputTail ?? null,
        outputTruncated: execution.outputTruncated ?? false,
        activity: payload.commandActivity ?? "ordinary",
      };
    }
    case "fetch":
    case "think":
      return { kind: payload.category, label };
    case "switchMode":
      return { kind: "switchMode", label };
    case "mcp":
      return { kind: "mcp", label, serverLabel: "MCP", toolLabel: label };
    case "subagent":
      return { kind: "subagent", label, activityId: null };
    default:
      return { kind: "other", label, rawKind: payload.category, summary: null };
  }
}

export async function getAcpChildTranscript(
  request: ChildTranscriptRequest,
): Promise<ChildTranscript> {
  const value: unknown = await invokeNative("get_acp_child_transcript", {
    request,
  });
  if (!isRecord(value)) throw new Error("Invalid child transcript response");
  if (value.kind === "tooLarge") return { kind: "tooLarge" };
  if (value.kind !== "current" && value.kind !== "fromEarlierSession")
    throw new Error("Invalid child transcript response");
  const snapshot = value.snapshot;
  if (
    !isRecord(snapshot) ||
    snapshot.childThreadId !== request.agentKey ||
    !safeIdentifier(snapshot.parentThreadId) ||
    !Array.isArray(snapshot.turns) ||
    snapshot.turns.length > 128 ||
    typeof snapshot.retentionLimited !== "boolean"
  )
    throw new Error("Invalid child transcript response");
  const turns: { id: string; status: string; items: AcpItem[] }[] = [];
  let total = 0;
  for (const turn of snapshot.turns) {
    if (
      !isRecord(turn) ||
      !Array.isArray(turn.items) ||
      !safeIdentifier(turn.id) ||
      !["inProgress", "completed", "interrupted", "failed"].includes(
        String(turn.status),
      )
    )
      throw new Error("Invalid child transcript response");
    total += turn.items.length;
    if (total > 1024) throw new Error("Invalid child transcript response");
    const turnId = safeIdentifier(turn.id);
    if (!turnId) throw new Error("Invalid child transcript response");
    const items = turn.items.map((item, index) =>
      itemForView(item, turnId, index, request),
    );
    if (items.some((item) => item === null))
      throw new Error("Invalid child transcript response");
    const admittedItems = items.filter(
      (item): item is AcpItem => item !== null,
    );
    turns.push({
      id: turnId,
      status: String(turn.status),
      items: admittedItems,
    });
  }
  return {
    kind: value.kind,
    retentionLimited: snapshot.retentionLimited,
    turns,
  };
}
