import {
  AGENT_DIFF_SIDE_BYTES,
  agentDiffKey,
  parseAgentDiffRef,
  type AgentDiffSnapshot,
  type GetAgentDiffRequest,
} from "../domain/agentDiff";
import { decimalU64Counter, isRecord } from "../domain/guards";
import { utf8Bytes } from "../domain/text";

const prefix = utf8Bytes("sage-file-diff-v1\0");
const decoder = new TextDecoder();

/** This module is loaded only for an actual immutable snapshot request. */
export async function fetchAgentDiffSnapshot(
  request: GetAgentDiffRequest,
  invoke: (request: GetAgentDiffRequest) => Promise<unknown>,
  failure: (kind: "invalidRequest" | "internal", message: string) => Error,
): Promise<AgentDiffSnapshot> {
  const reference = parseAgentDiffRef(request);
  if (
    !reference ||
    !decimalU64Counter(request.projectGeneration, { allowZero: true }) ||
    !decimalU64Counter(request.expectedProjectionRevision, { allowZero: true })
  ) {
    throw failure(
      "invalidRequest",
      "Invalid immutable file snapshot reference.",
    );
  }
  const boundedRequest = {
    ...reference,
    projectGeneration: request.projectGeneration,
    expectedProjectionRevision: request.expectedProjectionRevision,
  };
  const response = await invoke(boundedRequest);
  const snapshot = await parseAgentDiffSnapshot(response, boundedRequest);
  if (!snapshot)
    throw failure(
      "internal",
      "The immutable file snapshot failed its bounded identity or integrity check.",
    );
  return snapshot;
}

export async function parseAgentDiffSnapshot(
  value: unknown,
  request: GetAgentDiffRequest,
): Promise<AgentDiffSnapshot | null> {
  const reference = parseAgentDiffRef(value);
  const expected = parseAgentDiffRef(request);
  if (
    !reference ||
    !expected ||
    !isRecord(value) ||
    agentDiffKey(reference) !== agentDiffKey(expected) ||
    !decimalU64Counter(value.projectionRevision, { allowZero: true }) ||
    value.projectionRevision !== request.expectedProjectionRevision ||
    typeof value.oldText !== "string" ||
    typeof value.newText !== "string" ||
    value.oldText.length > AGENT_DIFF_SIDE_BYTES ||
    value.newText.length > AGENT_DIFF_SIDE_BYTES
  ) {
    return null;
  }
  const oldText = value.oldText,
    newText = value.newText;
  const projectionRevision = request.expectedProjectionRevision;
  const old = utf8Bytes(oldText),
    next = utf8Bytes(newText);
  if (
    old.length > AGENT_DIFF_SIDE_BYTES ||
    next.length > AGENT_DIFF_SIDE_BYTES ||
    old.length !== value.oldBytes ||
    next.length !== value.newBytes ||
    decoder.decode(old) !== oldText ||
    decoder.decode(next) !== newText
  )
    return null;

  // Same length-framed SHA-256 as native. Allocation is bounded by two admitted
  // sides plus the fixed prefix/length headers, never by untrusted byte counts.
  const bytes = new Uint8Array(prefix.length + 16 + old.length + next.length);
  bytes.set(prefix);
  const lengths = new DataView(bytes.buffer);
  lengths.setBigUint64(prefix.length, BigInt(old.length));
  bytes.set(old, prefix.length + 8);
  lengths.setBigUint64(prefix.length + 8 + old.length, BigInt(next.length));
  bytes.set(next, prefix.length + 16 + old.length);
  let digest: string;
  try {
    digest = Array.from(
      new Uint8Array(await crypto.subtle.digest("SHA-256", bytes)),
      (byte) => byte.toString(16).padStart(2, "0"),
    ).join("");
  } catch {
    return null;
  }
  if (digest !== reference.contentDigest) return null;
  return {
    ...reference,
    projectionRevision,
    oldText,
    newText,
    oldBytes: old.length,
    newBytes: next.length,
  };
}
