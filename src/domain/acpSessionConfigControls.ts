import type {
  AcpConfigChoice,
  AcpConfigOption,
  AcpSessionConfig,
} from "./acpSessionConfig";

/** Every mode the harness advertises, in its order; no mode id is special. */
export function sessionModeActions(config: AcpSessionConfig | null) {
  return config?.removed ? [] : (config?.modes?.availableModes ?? []);
}

export function flattenChoices(option: AcpConfigOption): AcpConfigChoice[] {
  if (option.type !== "select") return [];
  return option.options.flatMap((entry) =>
    "group" in entry ? entry.options : [entry],
  );
}

/** A select is usable only when its current value is among its choices. */
export function selectedConfigChoice(option: AcpConfigOption | undefined) {
  return option?.type === "select"
    ? flattenChoices(option).find(
        (choice) => choice.value === option.currentValue,
      )
    : undefined;
}

function restatesModes(
  option: AcpConfigOption,
  config: AcpSessionConfig,
): boolean {
  const modes = config.modes;
  if (!modes || option.type !== "select") return false;
  if (option.currentValue !== modes.currentModeId) return false;
  const choices = flattenChoices(option);
  if (choices.length !== modes.availableModes.length) return false;
  const modeIds = new Set(modes.availableModes.map((mode) => mode.id));
  return choices.every((choice) => modeIds.has(choice.value));
}

/**
 * The session's config without its modes, when the access control owns the
 * harness's session modes (Claude Code's permission modes). The generic mode
 * controls then step aside, so a mode is never chosen without the access
 * menu's rules and confirmations.
 */
export function withoutGovernedModes(
  config: AcpSessionConfig | null,
  governed: boolean,
): AcpSessionConfig | null {
  if (!config || !governed) return config;
  return {
    ...config,
    modes: null,
    configOptions: config.configOptions.filter(
      (option) => (option.category ?? option.id).toLocaleLowerCase() !== "mode",
    ),
  };
}

export function sessionConfigControls(
  config: AcpSessionConfig | null,
  group?: "model" | "other",
): AcpConfigOption[] {
  if (!config || config.removed) return [];
  return config.configOptions.filter((option) => {
    if (restatesModes(option, config)) return false;
    if (!group) return true;
    const modelRelated = ["model", "effort", "thought_level"].includes(
      (option.category ?? option.id).toLocaleLowerCase(),
    );
    return group === "model" ? modelRelated : !modelRelated;
  });
}
