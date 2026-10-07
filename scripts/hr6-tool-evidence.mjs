import { scanExceeded, turnSettled } from "./hr6-readiness.mjs";

export const SCRATCH_PROBE_TEXT = "read-only harness probe";

export const READ_PROBE = {
  kinds: ["read", "execute"],
  labelPattern: /(alpha\.txt|get-content|\bread\b)/iu,
  expectedText: SCRATCH_PROBE_TEXT,
};

const unsuccessfulToolStates = ["failed", "declined", "cancelled"];
const unsuccessfulTurnStates = ["Interrupted", "Cancelled", "Failed"];

export function toolResultEvidence(tool) {
  if (tool?.state !== "completed") return "none";
  if (
    tool.outputVisible === true &&
    typeof tool.outputText === "string" &&
    tool.outputText.trim().length > 0
  )
    return "output";
  if (Number.isInteger(tool.resources) && tool.resources > 0)
    return tool.resourcesExceeded === false ? "referencedResource" : "none";
  return "none";
}

function exactTextMatches(value, truncated, probe) {
  if (truncated !== false) return false;
  if (typeof value !== "string") return false;
  return value
    .split("\n")
    .some((line) => line.trim().replace(/^\d+\t/u, "") === probe.expectedText);
}

export function exactOutputMatches(tool, probe = READ_PROBE) {
  return exactTextMatches(tool?.outputText, tool?.outputTruncated, probe);
}

export function probeTools(turn, probe = READ_PROBE) {
  const tools = Array.isArray(turn?.tools) ? turn.tools : [];
  return tools.filter(
    (tool) =>
      probe.kinds.includes(tool?.kind) &&
      probe.labelPattern.test(
        typeof tool?.label === "string" ? tool.label : "",
      ),
  );
}

export function completedToolResults(turn, probe = READ_PROBE) {
  return probeTools(turn, probe)
    .map((tool) => ({ tool, evidence: toolResultEvidence(tool) }))
    .filter((entry) => entry.evidence === "output");
}

export function exactReplyMatches(turn, probe = READ_PROBE) {
  return exactTextMatches(
    turn?.assistantText,
    turn?.assistantTextTruncated,
    probe,
  );
}

export function toolEvidenceOutcome(state, turnId, probe = READ_PROBE) {
  const matched = Array.isArray(state?.turns)
    ? state.turns.filter((candidate) => candidate?.id === turnId)
    : [];
  if (matched.length > 1) return { kind: "turn-ambiguous" };
  const turn = matched[0];
  if (!turn || typeof turn.status !== "string") return null;
  if (state.alerts?.length > 0) return { kind: "error" };
  if (scanExceeded(state, turn)) return { kind: "scan-exceeded" };
  const results = completedToolResults(turn, probe);
  const proven = results.filter((entry) =>
    exactOutputMatches(entry.tool, probe),
  );
  if (proven.length > 0 && exactReplyMatches(turn, probe))
    return {
      kind: "tool-result",
      results: proven.map((entry) => ({
        kind: entry.tool.kind ?? null,
        label: entry.tool.label ?? null,
        evidence: entry.evidence,
      })),
    };
  if (!turnSettled(state)) return null;
  if (unsuccessfulTurnStates.includes(turn.status))
    return { kind: "turn-not-completed" };
  if (turn.status !== "Completed") return null;
  const matches = probeTools(turn, probe);
  if (matches.some((tool) => unsuccessfulToolStates.includes(tool?.state)))
    return { kind: "tool-unsuccessful" };
  const tools = Array.isArray(turn.tools) ? turn.tools : [];
  if (tools.length === 0) return { kind: "tool-absent" };
  if (matches.length === 0) return { kind: "tool-unrelated" };
  if (proven.length > 0) return { kind: "response-mismatch" };
  if (results.length > 0) return { kind: "tool-output-mismatch" };
  if (
    matches.some(
      (tool) =>
        typeof tool?.outputText === "string" &&
        tool.outputText.trim().length > 0 &&
        tool.outputVisible !== true,
    )
  )
    return { kind: "tool-output-hidden" };
  if (matches.some((tool) => toolResultEvidence(tool) === "referencedResource"))
    return { kind: "tool-resource-only" };
  return { kind: "tool-result-absent" };
}
