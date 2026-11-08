import type { AcpSessionConfig } from "./acpSessionConfig";
import type { SessionAccessModeCapability } from "./sessionAccessMode";

/**
 * The session's config without its modes, when the harness's access modes
 * carry its session modes (Claude Code's permission modes). The generic mode
 * controls then step aside, so a mode is never chosen without the access
 * menu's rules and confirmations. Loaded with the composer's options menu.
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
