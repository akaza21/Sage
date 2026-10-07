import { beforeEach, describe, expect, it } from "vitest";

import type { ModelOption } from "../domain/workbench";
import {
  cachedHarnessModels,
  HARNESS_MODEL_CACHE_LIMIT,
  rememberHarnessModels,
  resetHarnessModelCache,
} from "./harnessModelCache";

function models(id: string): ModelOption[] {
  return [{ id, name: id }];
}

describe("harnessModelCache", () => {
  beforeEach(() => {
    resetHarnessModelCache();
  });

  it("ignores a null identity on read and write", () => {
    rememberHarnessModels(null, models("ignored"));
    expect(cachedHarnessModels(null)).toBeNull();
  });

  it("returns the models remembered for an identity", () => {
    rememberHarnessModels("codex", models("gpt"));
    expect(cachedHarnessModels("codex")).toEqual(models("gpt"));
    expect(cachedHarnessModels("claude")).toBeNull();
  });

  it("evicts the least recently used identity past the limit", () => {
    for (let index = 0; index <= HARNESS_MODEL_CACHE_LIMIT; index += 1) {
      rememberHarnessModels(`harness-${index}`, models(`model-${index}`));
    }
    expect(cachedHarnessModels("harness-0")).toBeNull();
    expect(cachedHarnessModels("harness-1")).toEqual(models("model-1"));
    expect(cachedHarnessModels(`harness-${HARNESS_MODEL_CACHE_LIMIT}`)).toEqual(
      models(`model-${HARNESS_MODEL_CACHE_LIMIT}`),
    );
  });

  it("keeps a recently read identity alive when evicting", () => {
    for (let index = 0; index < HARNESS_MODEL_CACHE_LIMIT; index += 1) {
      rememberHarnessModels(`harness-${index}`, models(`model-${index}`));
    }
    expect(cachedHarnessModels("harness-0")).toEqual(models("model-0"));
    rememberHarnessModels("harness-fresh", models("model-fresh"));
    expect(cachedHarnessModels("harness-0")).toEqual(models("model-0"));
    expect(cachedHarnessModels("harness-1")).toBeNull();
  });

  it("refreshes recency when an identity is written again", () => {
    for (let index = 0; index < HARNESS_MODEL_CACHE_LIMIT; index += 1) {
      rememberHarnessModels(`harness-${index}`, models(`model-${index}`));
    }
    rememberHarnessModels("harness-0", models("model-0-updated"));
    rememberHarnessModels("harness-fresh", models("model-fresh"));
    expect(cachedHarnessModels("harness-0")).toEqual(models("model-0-updated"));
    expect(cachedHarnessModels("harness-1")).toBeNull();
  });
});
