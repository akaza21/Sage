import { useMemo } from "react";
import type {
  AcpConfigOption,
  AcpSessionConfig,
} from "../../domain/acpSessionConfig";
import type { ComposerModelChoice } from "./ChatModelPicker";

/**
 * The composer's model choices, the one the picker checks, and the one whose
 * choice changes nothing. A session model select owns all three. Otherwise
 * the picker checks the running model its label names. While no catalog is
 * available (none advertised, still loading, or a cache miss), a session's
 * confirmed model is the one known choice, so the picker lists it instead of
 * disabling, and choosing it only closes the menu. With a catalog the
 * unchanged choice is the chat's own request (the confirmed model only when
 * it requested none), so a harness substitution stays choosable.
 */
export function useComposerModelSelection(
  modelOption: AcpConfigOption | undefined,
  models: readonly ComposerModelChoice[],
  sessionConfig: AcpSessionConfig | null,
  label: string | null,
  requestedId: string | null,
) {
  const sessionModel = sessionConfig?.removed
    ? undefined
    : sessionConfig?.confirmedModel;
  const soleId = models.length === 0 ? sessionModel?.id : undefined;
  const soleName = label ?? sessionModel?.name ?? soleId;
  const catalog = useMemo(
    () => (soleId ? [{ id: soleId, name: soleName ?? soleId }] : models),
    [models, soleId, soleName],
  );
  if (modelOption?.type === "select") {
    const id = modelOption.currentValue;
    return { choices: modelOption, currentId: id, unchangedId: id };
  }
  return {
    choices: catalog,
    currentId: sessionConfig?.confirmedModel?.id ?? requestedId,
    unchangedId: soleId ?? requestedId ?? sessionModel?.id ?? null,
  };
}

export function useComposerModelChoices(
  source:
    | readonly ComposerModelChoice[]
    | Extract<AcpConfigOption, { type: "select" }>,
) {
  return useMemo(
    () =>
      "type" in source
        ? source.options.flatMap((entry) =>
            "group" in entry
              ? entry.options.map((choice) => ({
                  id: choice.value,
                  name: choice.name || choice.value,
                  group: entry.name,
                }))
              : [{ id: entry.value, name: entry.name || entry.value }],
          )
        : source,
    [source],
  );
}
