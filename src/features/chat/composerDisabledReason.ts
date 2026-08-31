import type { HarnessDiscoveryPhase } from "../../app/harnessSlice";

interface ComposerAvailability {
  actionBusy: boolean;
  activeTurn: boolean;
  availableBinding: boolean;
  canPrompt: boolean;
  harnessAvailable: boolean;
  harnessDiscoveryPhase: HarnessDiscoveryPhase;
  projectTransitioning: boolean;
  selectedHarness: boolean;
  useNativeAcp: boolean;
}

export function composerDisabledReason({
  actionBusy,
  activeTurn,
  availableBinding,
  canPrompt,
  harnessAvailable,
  harnessDiscoveryPhase,
  projectTransitioning,
  selectedHarness,
  useNativeAcp,
}: ComposerAvailability): string | null {
  if (projectTransitioning) return "Wait for the Project to finish loading.";
  if (actionBusy) return "Wait for the current action to finish.";
  if (activeTurn)
    return "Wait for the active Turn to finish, or cancel it when available.";
  if (!selectedHarness) return "Choose an available Harness before sending.";
  if (harnessDiscoveryPhase === "checking")
    return "Wait while Sage checks the selected Harness.";
  if (["signInRequired", "consent", "waiting"].includes(harnessDiscoveryPhase))
    return "Finish signing in before sending.";
  if (!harnessAvailable) return "The selected Harness is unavailable.";
  if (useNativeAcp && !availableBinding)
    return "No reviewed local binding is available for this Harness.";
  if (!canPrompt)
    return "The selected Harness cannot accept prompts in this session.";
  return null;
}
