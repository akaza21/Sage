import type { HarnessDefinition, ModelOption } from "../domain/workbench";
import { createStore } from "zustand/vanilla";

export const HARNESS_MODEL_CACHE_LIMIT = 16;

// Mounted readers share this bounded catalog; its map changes only on reset.
export const harnessModelCache = createStore(() => ({
  entries: new Map<string, ModelOption[]>(),
}));

export function harnessModelIdentity(
  harness: HarnessDefinition | null | undefined,
): string | null {
  return harness
    ? JSON.stringify([
        harness.id,
        harness.installState,
        harness.bindingId,
        harness.version,
        harness.boundVersion,
      ])
    : null;
}

export function cachedHarnessModels(
  identity: string | null,
): ModelOption[] | null {
  if (identity === null) return null;
  const { entries } = harnessModelCache.getState();
  const models = entries.get(identity);
  if (!models) return null;
  entries.delete(identity);
  entries.set(identity, models);
  return models;
}

export function rememberHarnessModels(
  identity: string | null,
  models: ModelOption[],
): void {
  if (identity === null) return;
  const { entries } = harnessModelCache.getState();
  entries.delete(identity);
  entries.set(identity, models);
  while (entries.size > HARNESS_MODEL_CACHE_LIMIT) {
    // An over-capacity map always has a first key.
    entries.delete(entries.keys().next().value!);
  }
  harnessModelCache.setState({ entries });
}

/** Forgets every listing, as when an account changes. */
export function resetHarnessModelCache(): void {
  harnessModelCache.setState({ entries: new Map() });
}
