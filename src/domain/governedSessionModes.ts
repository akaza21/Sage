import type { AcpSessionConfig } from "./acpSessionConfig";
import type { SessionAccessModeCapability } from "./sessionAccessMode";

/**
 * The generic mode controls step aside, so a mode is never chosen without the
 * access menu's rules and confirmations.
 */
export function withoutGovernedModes(
  config: AcpSessionConfig | null,
  accessModes: readonly SessionAccessModeCapability[] | undefined,
): AcpSessionConfig | null {
  if (!config || !accessModes?.some((mode) => mode.sessionModeId)) {
    return config;
  }
  return {
    ...config,
    modes: null,
    configOptions: config.configOptions.filter(
      (option) => (option.category ?? option.id).toLocaleLowerCase() !== "mode",
    ),
  };
}
