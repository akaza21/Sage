import { useState } from "react";

import type {
  AcpConfigOption,
  AcpSessionConfig,
} from "../../domain/acpSessionConfig";
import { composerOptionLabel } from "./composerChrome";

export function useConfirmedModelLabel(
  scope: string,
  modelOption: AcpConfigOption | undefined,
  models: readonly { id: string; name: string }[],
  sessionModel: AcpSessionConfig["confirmedModel"],
): string {
  const [confirmed, setConfirmed] = useState<{
    scope: string;
    modelId: string | null;
    label: string | null;
  } | null>(null);
  const previous = confirmed?.scope === scope ? confirmed : null;
  const modelId =
    modelOption?.type === "select"
      ? modelOption.currentValue
      : (sessionModel?.id ?? previous?.modelId ?? null);
  const label =
    composerOptionLabel(modelOption) ??
    sessionModel?.name ??
    models.find((model) => model.id === modelId)?.name ??
    (previous?.modelId === modelId ? previous.label : null) ??
    sessionModel?.id ??
    null;
  if (
    confirmed?.scope !== scope ||
    confirmed.modelId !== modelId ||
    confirmed.label !== label
  ) {
    setConfirmed({ scope, modelId, label });
  }
  return label ?? "Model";
}
