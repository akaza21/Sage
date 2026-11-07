import { useMemo } from "react";
import type {
  AcpConfigOption,
  AcpSessionConfig,
} from "../../domain/acpSessionConfig";
import type { ComposerModelChoice } from "./ChatModelPicker";

/**
 * The models the picker lists when the session has no model select, and the
 * one it marks current. While no catalog is available (none advertised, still
 * loading, or a cache miss), a session's confirmed model is the one known
 * choice, so the picker lists it as current instead of disabling. With a
 * catalog the current choice is the chat's own request (the confirmed model
 * only when it requested none), so a harness substitution stays choosable.
 */
export function useComposerModelCatalog(
  models: readonly ComposerModelChoice[],
  sessionModel: AcpSessionConfig["confirmedModel"],
  label: string | null,
  requestedId: string | null,
): {
  choices: readonly ComposerModelChoice[];
  currentId: string | null;
} {
  const soleId = models.length === 0 ? sessionModel?.id : undefined;
  const soleName = label ?? sessionModel?.name ?? soleId;
  const choices = useMemo(
    () => (soleId ? [{ id: soleId, name: soleName ?? soleId }] : models),
    [models, soleId, soleName],
  );
  return {
    choices,
    currentId: soleId ?? requestedId ?? sessionModel?.id ?? null,
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
