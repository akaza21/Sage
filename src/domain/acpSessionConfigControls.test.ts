import { describe, expect, it } from "vitest";

import { decodeSessionConfig } from "./acpSessionConfig";
import {
  flattenChoices,
  sessionConfigControls,
  sessionModeActions,
} from "./acpSessionConfigControls";
import { withoutGovernedModes } from "./governedSessionModes";

const claudeAccess = [
  {
    id: "plan" as const,
    label: "Plan",
    description: "Plans.",
    sessionModeId: "plan",
  },
];
const codexAccess = [
  { id: "ask" as const, label: "Ask", description: "Asks." },
];

const observedModes = {
  currentModeId: "default",
  availableModes: [
    { id: "default", name: "Manual" },
    { id: "acceptEdits", name: "Accept edits" },
    { id: "plan", name: "Plan" },
    { id: "auto", name: "Auto" },
    { id: "bypassPermissions", name: "Bypass permissions" },
  ],
};

const observedModeOption = {
  id: "mode",
  name: "Mode",
  category: "mode",
  type: "select",
  currentValue: "default",
  options: [
    { value: "default", name: "Manual" },
    { value: "acceptEdits", name: "Accept edits" },
    { value: "plan", name: "Plan" },
    { value: "auto", name: "Auto" },
    { value: "bypassPermissions", name: "Bypass permissions" },
  ],
};

const observedModelOption = {
  id: "model",
  name: "Model",
  category: "model",
  type: "select",
  currentValue: "default",
  options: [
    { value: "default", name: "Default (recommended)" },
    { value: "sonnet", name: "Sonnet 5" },
  ],
};

const decode = (modes: unknown, configOptions: unknown[]) =>
  decodeSessionConfig({
    sageAgentSessionId: "session-1",
    runtimeEpoch: "1",
    configRevision: "1",
    modes,
    configOptions,
  });

describe("sessionConfigControls", () => {
  it("drops the permission mode option the adapter restates alongside modes", () => {
    const config = decode(observedModes, [
      observedModeOption,
      observedModelOption,
    ]);
    expect(config).not.toBeNull();
    expect(sessionConfigControls(config).map((option) => option.id)).toEqual([
      "model",
    ]);
  });

  it("keeps a same-sized option whose values are not the mode ids", () => {
    const config = decode(observedModes, [
      {
        ...observedModeOption,
        id: "effort",
        name: "Effort",
        currentValue: "default",
        options: [
          { value: "default", name: "Default" },
          { value: "low", name: "Low" },
          { value: "medium", name: "Medium" },
          { value: "high", name: "High" },
          { value: "max", name: "Max" },
        ],
      },
    ]);
    expect(sessionConfigControls(config).map((option) => option.id)).toEqual([
      "effort",
    ]);
  });

  it("keeps the option when the harness advertises no modes at all", () => {
    const config = decode(null, [observedModeOption]);
    expect(sessionConfigControls(config).map((option) => option.id)).toEqual([
      "mode",
    ]);
  });

  it("projects nothing for a missing or removed configuration", () => {
    expect(sessionConfigControls(null)).toEqual([]);
    const removed = decodeSessionConfig({
      sageAgentSessionId: "session-1",
      runtimeEpoch: "1",
      configRevision: "2",
      removed: true,
      modes: null,
      configOptions: [],
    });
    expect(sessionConfigControls(removed)).toEqual([]);
  });

  it("flattens grouped choices and ignores boolean options", () => {
    const config = decode(null, [
      {
        id: "model",
        name: "Model",
        type: "select",
        currentValue: "sonnet",
        options: [
          {
            group: "anthropic",
            name: "Anthropic",
            options: [{ value: "sonnet", name: "Sonnet 5" }],
          },
          {
            group: "legacy",
            name: "Legacy",
            options: [{ value: "haiku", name: "Haiku 4.5" }],
          },
        ],
      },
      { id: "fast", name: "Fast mode", type: "boolean", currentValue: false },
    ]);
    const controls = sessionConfigControls(config);
    expect(flattenChoices(controls[0]).map((choice) => choice.value)).toEqual([
      "sonnet",
      "haiku",
    ]);
    expect(flattenChoices(controls[1])).toEqual([]);
  });
});

describe("sessionModeActions", () => {
  it("returns every advertised mode in order, giving no id special meaning", () => {
    const config = decode(observedModes, []);
    expect(sessionModeActions(config).map((mode) => mode.id)).toEqual([
      "default",
      "acceptEdits",
      "plan",
      "auto",
      "bypassPermissions",
    ]);
  });

  it("offers nothing once the session configuration is removed", () => {
    const config = decode(observedModes, []);
    expect(sessionModeActions({ ...config!, removed: true })).toEqual([]);
    expect(sessionModeActions(null)).toEqual([]);
  });
});

describe("withoutGovernedModes", () => {
  it("hands the permission modes to the access control, even a stale mode option", () => {
    // After a set_mode the adapter moves only its option, so the option no
    // longer restates the modes and would otherwise surface on its own.
    const config = decode(observedModes, [
      { ...observedModeOption, currentValue: "acceptEdits" },
      observedModelOption,
    ]);
    const governed = withoutGovernedModes(config, claudeAccess);
    expect(sessionModeActions(governed)).toEqual([]);
    expect(sessionConfigControls(governed).map((option) => option.id)).toEqual([
      "model",
    ]);
    expect(sessionConfigControls(config).map((option) => option.id)).toEqual([
      "mode",
      "model",
    ]);
  });

  it("leaves a harness whose access is not mode-based untouched", () => {
    const config = decode(observedModes, [observedModeOption]);
    expect(withoutGovernedModes(config, codexAccess)).toBe(config);
    expect(withoutGovernedModes(null, claudeAccess)).toBeNull();
  });
});
