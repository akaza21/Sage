import { expect, expectTypeOf, it } from "vitest";
import {
  harnessIconKind,
  harnessUnavailableReason,
  preferredAvailableHarness,
} from "./harness";
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

it.each([
  ["admissionIncomplete", "Codex is still being checked."],
  ["adapterAbsent", "Codex isn't set up for Sage yet."],
  ["adapterVersionUnreadable", "Codex has a version Sage can't read."],
  ["candidateInvalid", "Codex didn't pass Sage's checks."],
  [
    "capabilityProjectionUnavailable",
    "Codex has features Sage couldn't check.",
  ],
  ["developmentOnly", "Codex needs a development build."],
  ["driftDetected", "Codex changed since Sage checked it."],
  ["nodeAbsent", "Codex needs Node.js installed."],
  ["nodeEngineMismatch", "Codex needs a newer Node.js version."],
  ["nodeVersionUnreadable", "Codex needs a Node.js version Sage can read."],
  ["observationOnly", "Codex can't be verified on this computer."],
  ["platformUnsupported", "Codex isn't supported on this computer."],
  ["versionTooOld", "Codex needs an update for Sage."],
  ["unknown", "Codex can't start yet."],
  ["constructor", "Codex can't start yet."],
  ["__proto__", "Codex can't start yet."],
] as const)("explains %s using the harness display name", (kind, message) => {
  expect(harnessUnavailableReason(kind as never, "Codex")).toBe(message);
});

it("holds a remembered harness discovery still checks, and falls back once it fails", () => {
  const harness = (
    id: string,
    installState: HarnessDefinition["installState"],
    versionChecking = false,
  ) =>
    ({
      id,
      name: id,
      description: "",
      source: "builtIn",
      accountModelDiscovery: false,
      transport: "acp",
      installState,
      bindingId: installState === "available" ? `${id}-binding` : null,
      versionChecking,
    }) as HarnessDefinition;
  const fast = harness("fast", "available");
  expect(
    preferredAvailableHarness(
      [fast, harness("slow", "unavailable", true)],
      "slow",
    )?.id,
  ).toBe("slow");
  expect(
    preferredAvailableHarness([fast, harness("slow", "detecting")], "slow")?.id,
  ).toBe("slow");
  expect(
    preferredAvailableHarness([fast, harness("slow", "unavailable")], "slow")
      ?.id,
  ).toBe("fast");
});

it("names the Pi mark for Pi itself and the generic mark for its forks", () => {
  expect(
    ["Claude Code", "Codex", "OpenCode", "Pi", " pi ", "Oh My Pi", "Gemini"].map(
      harnessIconKind,
    ),
  ).toEqual(["claude", "codex", "opencode", "pi", "pi", undefined, undefined]);
});
