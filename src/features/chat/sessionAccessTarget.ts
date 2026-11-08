import { useMemo } from "react";
import type { SessionAccessTarget } from "../../app/useSessionAccessSelection";
import type { AcpSessionConfig } from "../../domain/acpSessionConfig";
import { withoutGovernedModes } from "../../domain/acpSessionConfigControls";
import {
  accessGovernsSessionModes,
  type SessionAccessModeCapability,
} from "../../domain/sessionAccessMode";

/** The live session an access choice applies to, from its reported config. */
export function sessionAccessTarget(
  config: AcpSessionConfig | null | undefined,
): SessionAccessTarget | null {
  if (!config || config.removed) return null;
  return {
    sessionId: config.sageAgentSessionId,
    availableModeIds:
      config.modes?.availableModes.map((mode) => mode.id) ?? null,
  };
}

/**
 * The session config the composer's generic controls show. Session modes the
 * access control owns are left to it.
 */
export function useComposerConfig(
  config: AcpSessionConfig | null,
  harness: { accessModes?: readonly SessionAccessModeCapability[] } | undefined,
) {
  const governed = accessGovernsSessionModes(harness?.accessModes);
  return useMemo(
    () => withoutGovernedModes(config, governed),
    [config, governed],
  );
}
