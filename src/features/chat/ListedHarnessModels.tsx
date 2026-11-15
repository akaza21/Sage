import { useEffect } from "react";

import {
  harnessModelGeneration,
  harnessModelIdentity,
  rememberHarnessModels,
} from "../../app/harnessModelCache";
import type { HarnessDefinition, ModelOption } from "../../domain/workbench";
import { desktop } from "../../services/desktop";

/** The harness lists nothing new this soon after a failed listing. */
const RELIST_AFTER_MS = 60_000;

/**
 * Asks `harness` for the models it lists before a session, for a chat whose
 * harness is not the one the workbench selected and whose models the cache
 * does not hold, and remembers them. A harness that lists nothing is asked
 * once more after its own backoff.
 */
export function ListedHarnessModels({
  harness,
  onListed,
}: {
  harness: HarnessDefinition;
  onListed: () => void;
}) {
  const identity = harnessModelIdentity(harness);
  const generation = harnessModelGeneration();
  useEffect(() => {
    if (harness.installState !== "available") return;
    let current = true;
    let tries = 2;
    let relist: ReturnType<typeof setTimeout> | undefined;
    const list = () =>
      desktop
        .listHarnessModels(harness.id)
        .catch((): ModelOption[] => [])
        .then((models) => {
          // Models listed for an account that has since changed are dropped.
          if (!current || harnessModelGeneration() !== generation) return;
          if (models.length > 0) {
            rememberHarnessModels(identity, models);
            onListed();
          } else if (--tries > 0) relist = setTimeout(list, RELIST_AFTER_MS);
        });
    void list();
    return () => {
      current = false;
      clearTimeout(relist);
    };
  }, [generation, harness.id, harness.installState, identity, onListed]);
  return null;
}
