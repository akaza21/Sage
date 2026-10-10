import { useState } from "react";

import type { AcpConfigOption } from "../../domain/acpSessionConfig";
import { composerOptionLabel } from "./composerChrome";

export function useConfirmedModelLabel(
  scope: string,
  selectedModelId: string | null,
  modelOption: AcpConfigOption | undefined,
  models: readonly { id: string; name: string }[],
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
      : (previous?.modelId ?? selectedModelId);
  const label =
    composerOptionLabel(modelOption) ??
    models.find((model) => model.id === modelId)?.name ??
    (previous?.modelId === modelId ? previous.label : null);
  if (
    confirmed?.scope !== scope ||
    confirmed.modelId !== modelId ||
    confirmed.label !== label
  ) {
    setConfirmed({ scope, modelId, label });
  }
  return label ?? "Model";
}
