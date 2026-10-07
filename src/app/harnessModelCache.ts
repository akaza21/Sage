import type { HarnessDefinition, ModelOption } from "../domain/workbench";

export const HARNESS_MODEL_CACHE_LIMIT = 16;

const entries = new Map<string, ModelOption[]>();

export function harnessModelIdentity(
  harness: HarnessDefinition | undefined,
): string | null {
  return harness
    ? JSON.stringify([
        harness.id,
        harness.installState,
        harness.bindingId,
        harness.version ?? null,
        harness.boundVersion ?? null,
      ])
    : null;
}

export function cachedHarnessModels(
  identity: string | null,
): ModelOption[] | null {
  if (identity === null) return null;
  const models = entries.get(identity);
  if (models === undefined) return null;
  entries.delete(identity);
  entries.set(identity, models);
  return models;
}

export function rememberHarnessModels(
  identity: string | null,
  models: ModelOption[],
): void {
  if (identity === null) return;
  entries.delete(identity);
  entries.set(identity, models);
  while (entries.size > HARNESS_MODEL_CACHE_LIMIT) {
    const oldest = entries.keys().next();
    if (oldest.done === true) break;
    entries.delete(oldest.value);
  }
}

export function resetHarnessModelCache(): void {
  entries.clear();
}
