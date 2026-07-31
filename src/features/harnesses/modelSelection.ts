import type { ModelOption } from "../../domain/workbench";

export function preferredModelId(
  models: ModelOption[],
  previousModelId: string | null,
): string | null {
  if (models.length === 0) {
    return null;
  }

  if (previousModelId && models.some((model) => model.id === previousModelId)) {
    return previousModelId;
  }

  return models[0]?.id ?? null;
}
