import type {
  ProjectGeneration,
  ProjectionRevision,
  RuntimeEpoch,
  SageAgentSessionId,
  SageChatId,
  SageItemId,
  SageProjectId,
  SageTurnId,
} from "./acp";
import { boundedIdentifier, decimalU64Counter, isRecord } from "./guards";
import { utf8ByteLength } from "./text";

export {
  AGENT_DIFF_SIDE_BYTES,
  MAX_AGENT_DIFFS_PER_TURN,
} from "./agentDiffIdentity";

export type FilePairMetadata =
  | {
      pairState?: undefined;
      snapshotRevision?: undefined;
      oldBytes?: undefined;
      newBytes?: undefined;
      contentDigest?: undefined;
    }
  | {
      pairState: "unavailable" | "summaryTruncated";
      snapshotRevision: "0";
      oldBytes: null;
      newBytes: null;
      contentDigest: null;
    }
  | {
      pairState: "available";
      snapshotRevision: string;
      oldBytes: number;
      newBytes: number;
      contentDigest: string;
    };

/** Immutable placement identity; durable tabs retain these fields, never text. */
export interface AgentDiffRef {
  projectId: SageProjectId;
  chatId: SageChatId;
  sessionId: SageAgentSessionId;
  runtimeEpoch: RuntimeEpoch;
  turnId: SageTurnId;
  itemId: SageItemId;
  diffOrdinal: 0;
  snapshotRevision: string;
  rootId: string;
  relativePath: string;
  contentDigest: string;
}

export interface GetAgentDiffRequest extends AgentDiffRef {
  projectGeneration: ProjectGeneration;
  expectedProjectionRevision: ProjectionRevision;
}

export interface AgentDiffSnapshot extends AgentDiffRef {
  projectionRevision: ProjectionRevision;
  oldText: string;
  newText: string;
  oldBytes: number;
  newBytes: number;
}

export function parseAgentDiffRef(value: unknown): AgentDiffRef | null {
  if (!isRecord(value)) return null;
  const ids = {
    projectId: boundedIdentifier(value.projectId),
    chatId: boundedIdentifier(value.chatId),
    sessionId: boundedIdentifier(value.sessionId),
    turnId: boundedIdentifier(value.turnId),
    itemId: boundedIdentifier(value.itemId),
    rootId: boundedIdentifier(value.rootId),
  };
  const runtimeEpoch = decimalU64Counter(value.runtimeEpoch, {
    allowZero: false,
  });
  const snapshotRevision = decimalU64Counter(value.snapshotRevision, {
    allowZero: false,
  });
  const path = value.relativePath;
  if (
    Object.values(ids).some((id) => id === null) ||
    !runtimeEpoch ||
    !snapshotRevision ||
    value.diffOrdinal !== 0 ||
    typeof path !== "string" ||
    path.length === 0 ||
    path.length > 16_384 ||
    utf8ByteLength(path) > 16_384 ||
    path.includes("\0") ||
    path.startsWith("/") ||
    path.includes("\\") ||
    (path.charCodeAt(0) < 128 && path[1] === ":") ||
    path
      .split("/")
      .some((part) => part === "" || part === "." || part === "..") ||
    typeof value.contentDigest !== "string" ||
    !/^[0-9a-f]{64}$/.test(value.contentDigest)
  ) {
    return null;
  }
  return {
    ...ids,
    runtimeEpoch,
    snapshotRevision,
    diffOrdinal: 0,
    relativePath: path,
    contentDigest: value.contentDigest,
  } as AgentDiffRef;
}

export { agentDiffKey } from "./agentDiffIdentity";
