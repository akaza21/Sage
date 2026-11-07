import { useMemo } from "react";
import type {
  AcpConfigOption,
  AcpSessionConfig,
} from "../../domain/acpSessionConfig";
import type { ComposerModelChoice } from "./ChatModelPicker";

/**
 * The models the picker lists when the session has no model select. A session
 * that confirms its model while the harness advertises no catalog has exactly
 * one model, so the picker lists that model as selected instead of disabling.
 */
export function useComposerModelCatalog(
  models: readonly ComposerModelChoice[],
  sessionModel: AcpSessionConfig["confirmedModel"],
  label: string | null,
): readonly ComposerModelChoice[] {
  const soleId = models.length === 0 ? sessionModel?.id : undefined;
  const soleName = label ?? sessionModel?.name ?? soleId;
  return useMemo(
    () => (soleId ? [{ id: soleId, name: soleName ?? soleId }] : models),
    [models, soleId, soleName],
  );
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
