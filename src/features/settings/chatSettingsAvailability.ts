import type { WorkbenchState } from "../../app/store";

export const chatSettingsDisabled = (state: WorkbenchState) =>
  !state.project ||
  state.projectTransitioning ||
  state.harnessDiscoveryPhase === "checking" ||
  state.harnessDiscoveryPhase === "waiting";

export const modelSettingsDisabled = (state: WorkbenchState) =>
  chatSettingsDisabled(state) ||
  state.harnessDiscoveryPhase !== "ready" ||
  !state.models.length;
