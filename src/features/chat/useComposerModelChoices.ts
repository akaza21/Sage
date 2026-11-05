import { useMemo } from "react";
import type { AcpConfigOption } from "../../domain/acpSessionConfig";
import type { ComposerModelChoice } from "./ChatModelPicker";

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
