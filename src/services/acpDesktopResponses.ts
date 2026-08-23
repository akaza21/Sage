import type { CodexDiscoveryResponse } from "../domain/acp";

export function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null;
}

export function safeIdentifier(value: unknown): string | null {
  if (
    typeof value !== "string" ||
    value.length === 0 ||
    value.length > 512 ||
    value.includes("\0")
  ) {
    return null;
  }
  return new TextEncoder().encode(value).byteLength <= 512 ? value : null;
}

export function safeCounter(value: unknown, allowZero: boolean): string | null {
  if (typeof value !== "string" || !/^(0|[1-9][0-9]{0,18})$/.test(value)) {
    return null;
  }
  if (value.length === 19 && value > "9223372036854775807") {
    return null;
  }
  return allowZero || value !== "0" ? value : null;
}

export function isOneOf<const Value extends string>(
  value: unknown,
  allowed: readonly Value[],
): value is Value {
  return typeof value === "string" && allowed.includes(value as Value);
}

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
    !isOneOf(value.account, ["managedChatgpt", "signedOut"] as const) ||
    value.models.length > 128
  ) {
    return null;
  }
  const ids = new Set<string>();
  let defaults = 0;
  const models = [];
  for (const candidate of value.models) {
    if (!isRecord(candidate)) return null;
    const id = safeIdentifier(candidate.id);
    const name = safeIdentifier(candidate.name);
    if (
      !id ||
      !name ||
      typeof candidate.isDefault !== "boolean" ||
      ids.has(id)
    ) {
      return null;
    }
    ids.add(id);
    defaults += Number(candidate.isDefault);
    models.push({ id, name, isDefault: candidate.isDefault });
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
  ) {
    return null;
  }
  return {
    projectId,
    projectGeneration,
    rootId,
    bindingId,
    runtimeEpoch,
    account: value.account,
    models,
    defaultModelId,
  } as unknown as CodexDiscoveryResponse;
}
