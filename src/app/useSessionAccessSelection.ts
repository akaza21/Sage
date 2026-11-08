import { useSessionAccessModeStore } from "./sessionAccessModeStore";
import {
  sessionAccessModeAvailability,
  sessionAccessModeCandidates,
  type SessionAccessModeCapability,
} from "../domain/sessionAccessMode";

export function useSessionAccessSelection(
  bindingId: string | null | undefined,
  capabilities: readonly SessionAccessModeCapability[] | undefined,
) {
  const mode = useSessionAccessModeStore((state) =>
    bindingId ? state.modes[bindingId] : undefined,
  );
  const ready = useSessionAccessModeStore((state) => state.ready);
  const error = useSessionAccessModeStore((state) => state.error);
  const setMode = useSessionAccessModeStore((state) => state.setMode);
  const candidates = sessionAccessModeCandidates(capabilities);
  const effective = sessionAccessModeAvailability(mode, candidates).effective;
  return {
    ready,
    error,
    setMode,
    candidates,
    effective,
    reason: !ready
      ? "Permission mode could not be loaded."
      : (error ??
        (effective === null
          ? "Choose a supported access mode before sending."
          : null)),
  };
}
