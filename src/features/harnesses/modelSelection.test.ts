import { describe, expect, it } from "vitest";

import { preferredModelId } from "../../app/harnessSlice";

const models = [
  { id: "fast", name: "Fast" },
  { id: "deep", name: "Deep" },
];

describe("preferredModelId", () => {
  it("retains a model only when the selected harness advertises it", () => {
    expect(preferredModelId(models, "deep")).toBe("deep");
    expect(preferredModelId(models, "foreign-model")).toBe("fast");
  });

  it("uses the harness default state when no models are advertised", () => {
    expect(preferredModelId([], "deep")).toBeNull();
  });
});
