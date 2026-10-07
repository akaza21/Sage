import { expect, expectTypeOf, it } from "vitest";
import { harnessUnavailableReason } from "./harness";
import type { HarnessDefinition, ModelOption } from "./harness";

it("keeps native executable paths out of the WebView harness DTO", () => {
  expectTypeOf<"executablePath">().not.toExtend<keyof HarnessDefinition>();
  expectTypeOf<HarnessDefinition["version"]>().toEqualTypeOf<
    string | null | undefined
  >();
  const harness: HarnessDefinition = {
    id: "pi",
    name: "Pi",
    description: "Local CLI discovery; launch integration remains pending.",
    source: "builtIn",
    accountModelDiscovery: false,
    transport: "acp",
    installState: "unavailable",
    bindingId: null,
    version: "1.2.3",
  };
  expect(Object.keys(harness).sort()).toEqual([
    "accountModelDiscovery",
    "bindingId",
    "description",
    "id",
    "installState",
    "name",
    "source",
    "transport",
    "version",
  ]);
  expect(harness.bindingId).toBeNull();
  expect(harness.installState).toBe("unavailable");
});

it("distinguishes a known model context window from an unknown one", () => {
  expectTypeOf<ModelOption["contextWindow"]>().toEqualTypeOf<
    number | undefined
  >();
  const known: ModelOption = {
    id: "model-known",
    name: "Known",
    contextWindow: 272_000,
  };
  const unknown: ModelOption = { id: "model-unknown", name: "Unknown" };

  expect(known.contextWindow).toBe(272_000);
  expect("contextWindow" in unknown).toBe(false);
  expect(unknown.contextWindow).toBeUndefined();
});

it("uses the admission refusal text for unavailable harness controls", () => {
  expect(harnessUnavailableReason("nodeEngineMismatch")).toBe(
    "The installed Node.js is older than this harness requires.",
  );
  expect(harnessUnavailableReason("unknown" as never)).toBe(
    "Sage did not admit this harness and will not start it.",
  );
});
