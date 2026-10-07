import type {
  AcpConfigChoice,
  AcpConfigOption,
  AcpSessionConfig,
} from "./acpSessionConfig";

export function flattenChoices(option: AcpConfigOption): AcpConfigChoice[] {
  if (option.type !== "select") return [];
  return option.options.flatMap((entry) =>
    "group" in entry ? entry.options : [entry],
  );
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

export function sessionConfigControls(
  config: AcpSessionConfig | null,
): AcpConfigOption[] {
  if (!config || config.removed) return [];
  return config.configOptions.filter(
    (option) => !restatesModes(option, config),
  );
}
