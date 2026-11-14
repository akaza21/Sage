// Decoders for a chat's stored session-config state: the config shown while
// a session has no live one, and the notice about a stored choice it could not
// take back. Only chat page responses carry these, so they live apart from the
// live config decoder that the startup bundle loads.
import {
  decodeSessionConfig,
  type AcpRestoredSessionConfig,
  type AcpSessionConfigNotice,
} from "./acpSessionConfig";
import { boundedIdentifier, isRecord } from "./guards";

const record = (v: unknown): v is Record<string, unknown> =>
  isRecord(v) && !Array.isArray(v);
/** A non-empty string of at most 1024 UTF-8 bytes with no NUL. */
const text = (v: unknown): v is string => boundedIdentifier(v, 1024) !== null;

export function decodeRestoredSessionConfig(
  v: unknown,
): AcpRestoredSessionConfig | null {
  if (
    !record(v) ||
    "runtimeEpoch" in v ||
    "configRevision" in v ||
    (v.awaiting !== "resume" &&
      v.awaiting !== "report" &&
      v.awaiting !== "start")
  )
    return null;
  const awaiting = v.awaiting;
  const decoded = decodeSessionConfig({
    ...v,
    runtimeEpoch: "0",
    configRevision: "1",
  });
  if (!decoded || decoded.removed) return null;
  const { sageAgentSessionId, modes, configOptions } = decoded;
  return {
    sageAgentSessionId,
    awaiting,
    modes,
    configOptions,
    ...(decoded.confirmedModel
      ? { confirmedModel: decoded.confirmedModel }
      : {}),
    ...(decoded.confirmedEffort === undefined
      ? {}
      : { confirmedEffort: decoded.confirmedEffort }),
  };
}
export function decodeSavedChoiceNotice(
  v: unknown,
): AcpSessionConfigNotice | null {
  if (
    !record(v) ||
    Object.keys(v).length !== 4 ||
    !text(v.configId) ||
    !text(v.saved) ||
    !text(v.current) ||
    (v.reason !== "notOffered" && v.reason !== "rejected")
  )
    return null;
  return {
    configId: v.configId,
    saved: v.saved,
    current: v.current,
    reason: v.reason,
  };
}
