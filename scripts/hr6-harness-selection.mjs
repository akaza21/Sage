export const HARNESS_SELECTION_STATES = Object.freeze([
  "selected",
  "absent",
  "disabled",
  "readiness-timeout",
  "alternative",
]);

const NOT_DRIVABLE_STATES = Object.freeze(["absent", "disabled"]);

export function harnessSelectionOutcome(state) {
  if (typeof state !== "string" || !HARNESS_SELECTION_STATES.includes(state))
    throw new Error("hr6-harness-selection-invalid");
  if (state === "selected") return { kind: "selected", state };
  if (NOT_DRIVABLE_STATES.includes(state))
    return { kind: "harness-not-drivable", state };
  return { kind: "harness-not-ready", state };
}

export function includeOpenCodeRequested(values) {
  if (typeof values !== "object" || values === null)
    throw new Error("hr6-arguments-invalid");
  const raw = values["--include-opencode"];
  if (raw === undefined) return false;
  if (raw === "true") return true;
  if (raw === "false") return false;
  throw new Error("hr6-arguments-invalid");
}
