import type {
  CodexDiscoveryModel,
  CodexDiscoveryResponse,
} from "../domain/acp";
import {
  isRecord,
  isOneOf,
  safeIdentifier,
  safeCounter,
} from "./acpDesktopGuards";

export function safeCodexDiscovery(
  value: unknown,
): CodexDiscoveryResponse | null {
  if (!isRecord(value) || !Array.isArray(value.models)) return null;
  const projectId = safeIdentifier(value.projectId);
  const projectGeneration = safeCounter(value.projectGeneration, true);
  const rootId = safeIdentifier(value.rootId);
  const bindingId = safeIdentifier(value.bindingId);
  const runtimeEpoch = safeCounter(value.runtimeEpoch, false);
  if (
    !projectId ||
    !projectGeneration ||
    !rootId ||
    !bindingId ||
    !runtimeEpoch ||
    !isOneOf(value.account, [
      "managedChatgpt",
      "apiKey",
      "providerManaged",
      "signedOut",
    ] as const) ||
    value.models.length > 128
  )
    return null;
  const ids = new Set<string>();
  let defaults = 0;
  const models: CodexDiscoveryModel[] = [];
  for (const candidate of value.models) {
    if (!isRecord(candidate)) return null;
    const id = safeIdentifier(candidate.id);
    const name = safeIdentifier(candidate.name);
    const effortLevels =
      candidate.effortLevels === undefined
        ? []
        : Array.isArray(candidate.effortLevels)
          ? candidate.effortLevels.map((level) => safeIdentifier(level))
          : null;
    if (
      !id ||
      !name ||
      typeof candidate.isDefault !== "boolean" ||
      ids.has(id) ||
      !effortLevels ||
      effortLevels.some((level) => !level)
    )
      return null;
    ids.add(id);
    defaults += Number(candidate.isDefault);
    models.push({
      id: id as CodexDiscoveryModel["id"],
      name,
      isDefault: candidate.isDefault,
      ...(effortLevels.length > 0
        ? { effortLevels: effortLevels as string[] }
        : {}),
    });
  }
  const defaultModelId =
    value.defaultModelId === null ? null : safeIdentifier(value.defaultModelId);
  if (
    (value.account === "signedOut" &&
      (models.length !== 0 || defaultModelId !== null)) ||
    (value.account === "managedChatgpt" && models.length === 0) ||
    defaults > 1 ||
    (value.defaultModelId !== null && !defaultModelId) ||
    (defaultModelId !== null && !ids.has(defaultModelId)) ||
    (defaults === 0 && defaultModelId !== null) ||
    (defaults === 1 &&
      !models.some((model) => model.id === defaultModelId && model.isDefault))
  )
    return null;
  return {
    projectId: projectId as CodexDiscoveryResponse["projectId"],
    projectGeneration:
      projectGeneration as CodexDiscoveryResponse["projectGeneration"],
    rootId,
    bindingId: bindingId as CodexDiscoveryResponse["bindingId"],
    runtimeEpoch: runtimeEpoch as CodexDiscoveryResponse["runtimeEpoch"],
    account: value.account,
    models,
    defaultModelId: defaultModelId as CodexDiscoveryResponse["defaultModelId"],
  };
}
