import type { AcpItem } from "../domain/acp";
import { truncateUtf8, utf8ByteLength } from "../domain/text";
import { isRecord, safeIdentifier } from "./acpDesktopGuards";
import { safeItem } from "./acpDesktopResponseItems";
import { invokeNative, isTauriRuntime } from "../ports/nativeInvoke";

/**
 * Child-only projection notices (Rust `ACP_CHILD_PROJECTION_CHANGED_EVENT`):
 * a child transcript changed while the parent page did not, so only open
 * Subagent views re-read and the parent page is not reloaded.
 */
export async function subscribeChildProjectionChanges(
  projectId: string,
  chatId: string,
  onEpoch: (epoch: number) => void,
): Promise<() => void> {
  if (!isTauriRuntime()) return () => undefined;
  const { listen } = await import("@tauri-apps/api/event");
  return listen<unknown>("acp-child-projection-changed-v1", ({ payload }) => {
    if (
      isRecord(payload) &&
      payload.projectId === projectId &&
      payload.chatId === chatId &&
      Number.isSafeInteger(payload.childProjectionEpoch)
    )
      onEpoch(payload.childProjectionEpoch as number);
  });
}

export interface ChildTranscriptRequest {
  projectId: string;
  projectGeneration: string;
  chatId: string;
  sessionId: string;
  runtimeEpoch: string;
  itemId: string;
  agentKey: string;
}

export type TranscriptOwner = Pick<
  ChildTranscriptRequest,
  "projectId" | "chatId" | "sessionId" | "runtimeEpoch"
>;

export type ChildTranscript =
  | {
      kind: "current" | "fromEarlierSession";
      retentionLimited: boolean;
      turns: TranscriptTurn[];
      /** The model the spawn requested, not necessarily the one it ran. */
      spawnModel?: { model: string; reasoningEffort?: string };
    }
  | {
      kind:
        "tooLarge" | "stale" | "rejected" | "recoveryRequired" | "unavailable";
    };

function itemForView(
  value: unknown,
  turnId: string,
  index: number,
  request: TranscriptOwner,
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

export type TranscriptTurn = { id: string; status: string; items: AcpItem[] };

/**
 * Native Turns (a child transcript or an imported session's history) in
 * the shared item shapes, or null when any Turn or item is malformed or the
 * bounds (128 Turns, 1024 items) are exceeded.
 */
export function turnsForView(
  value: unknown,
  owner: TranscriptOwner,
): TranscriptTurn[] | null {
  if (!Array.isArray(value) || value.length > 128) return null;
  const turns: TranscriptTurn[] = [];
  let total = 0;
  for (const turn of value) {
    const turnId = isRecord(turn) ? safeIdentifier(turn.id) : null;
    if (
      !isRecord(turn) ||
      !turnId ||
      !Array.isArray(turn.items) ||
      !["inProgress", "completed", "interrupted", "failed"].includes(
        String(turn.status),
      )
    )
      return null;
    total += turn.items.length;
    if (total > 1024) return null;
    const items = turn.items.map((item, index) =>
      itemForView(item, turnId, index, owner),
    );
    if (items.some((item) => item === null)) return null;
    turns.push({
      id: turnId,
      status: String(turn.status),
      items: items.filter((item): item is AcpItem => item !== null),
    });
  }
  return turns;
}

export async function getAcpChildTranscript(
  request: ChildTranscriptRequest,
): Promise<ChildTranscript> {
  let value: unknown;
  try {
    value = await invokeNative("get_acp_child_transcript", { request });
  } catch (error) {
    const kind = isRecord(error) ? error.kind : null;
    return {
      kind:
        kind === "conflict"
          ? "stale"
          : kind === "rejected"
            ? "rejected"
            : kind === "recoveryRequired"
              ? "recoveryRequired"
              : "unavailable",
    };
  }
  if (!isRecord(value)) throw new Error("Invalid child transcript response");
  if (value.kind === "tooLarge") return { kind: "tooLarge" };
  if (value.kind !== "current" && value.kind !== "fromEarlierSession")
    throw new Error("Invalid child transcript response");
  const snapshot = value.snapshot;
  if (
    !isRecord(snapshot) ||
    snapshot.childThreadId !== request.agentKey ||
    !safeIdentifier(snapshot.parentThreadId) ||
    typeof snapshot.retentionLimited !== "boolean"
  )
    throw new Error("Invalid child transcript response");
  const turns = turnsForView(snapshot.turns, request);
  if (!turns) throw new Error("Invalid child transcript response");
  const spawn = isRecord(value.spawnModel) ? value.spawnModel : {};
  const model = printable(spawn.model, 256);
  const reasoningEffort = printable(spawn.reasoningEffort, 32);
  return {
    kind: value.kind,
    retentionLimited: snapshot.retentionLimited,
    turns,
    ...(model
      ? {
          spawnModel: {
            model,
            ...(reasoningEffort ? { reasoningEffort } : {}),
          },
        }
      : {}),
  };
}

/** Optional header chrome: malformed text is dropped, never displayed. */
function printable(value: unknown, maximumBytes: number): string | null {
  return typeof value === "string" &&
    value.length > 0 &&
    utf8ByteLength(value) <= maximumBytes &&
    // eslint-disable-next-line no-control-regex
    !/[\u0000-\u001f\u007f]/.test(value)
    ? value
    : null;
}
