import { expect, expectTypeOf, it } from "vitest";
import type { HarnessDefinition } from "./harness";

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
