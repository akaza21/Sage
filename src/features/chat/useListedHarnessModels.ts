import { useEffect, useState } from "react";

import {
  cachedHarnessModels,
  harnessModelIdentity,
  rememberHarnessModels,
} from "../../app/harnessModelCache";
import type { HarnessDefinition, ModelOption } from "../../domain/workbench";
import { desktop } from "../../services/desktop";

/**
 * The models `harness` lists before a session, for a chat whose harness is
 * not the one the workbench selected: from the model cache, or asked of the
 * harness once when the cache has none. `null` while none are known.
 */
export function useListedHarnessModels(
  harness: HarnessDefinition | undefined,
): ModelOption[] | null {
  const identity = harnessModelIdentity(harness);
  const harnessId = harness?.installState === "available" ? harness.id : null;
  const [listed, setListed] = useState<{
    identity: string;
    models: ModelOption[];
  } | null>(null);
  const cached = cachedHarnessModels(identity);
  const missing = harnessId !== null && identity !== null && !cached;
  useEffect(() => {
    if (!missing || harnessId === null || identity === null) return;
    let current = true;
    desktop
      .listHarnessModels(harnessId)
      .then((models) => {
        if (models.length === 0) return;
        rememberHarnessModels(identity, models);
        if (current) setListed({ identity, models });
      })
      .catch(() => undefined);
    return () => {
      current = false;
    };
  }, [harnessId, identity, missing]);
  return cached ?? (listed?.identity === identity ? listed.models : null);
}
