import type { RailAttention } from "./agentRailInteractions";
import type { AgentRailTrailingState } from "./agentRailStatusView";

// Rail rows only: kept out of the shared status modules so the wording
// loads with the rows.

/** The pill of a chat waiting on the user; a mounted chat's own approval counts. */
export const railPill = (mounted: boolean, attention?: RailAttention) =>
  attention === "input"
    ? "Needs input"
    : mounted || attention === "approval"
      ? "Awaiting approval"
      : "";

/** A failure that needs a sign-in reads "Sign in" rather than "Failed". */
export const railTrailing = (
  state: AgentRailTrailingState | null,
  attention?: RailAttention,
) =>
  state?.kind === "failed" && attention === "signIn"
    ? { ...state, label: "Sign in" }
    : state;
