import { describe, expect, it } from "vitest";
import {
  decodeSessionConfig,
  decodeSessionConfigNotice,
} from "./acpSessionConfig";

const choice = { value: "high", name: "High" };
const select = {
  id: "effort",
  name: "Effort",
  category: "thought_level",
  type: "select",
  currentValue: "high",
  options: [choice],
};
const overlay = () => ({
  sageAgentSessionId: "session-a",
  runtimeEpoch: "2",
  configRevision: "1",
  modes: { currentModeId: "ask", availableModes: [{ id: "ask", name: "Ask" }] },
  configOptions: [select],
});

describe("session config boundary", () => {
  it("admits bounded confirmed effort without advertising selectable options", () => {
    const config = {
      ...overlay(),
      modes: null,
      configOptions: [],
      confirmedEffort: "high",
    };
    expect(decodeSessionConfig(config)).toMatchObject({
      confirmedEffort: "high",
      configOptions: [],
    });
    for (const confirmedEffort of ["", "x".repeat(513), "\0", 1, null]) {
      expect(decodeSessionConfig({ ...config, confirmedEffort })).toBeNull();
    }
    expect(decodeSessionConfig({ ...config, removed: true })).toBeNull();
    const configOptions = Array.from({ length: 128 }, (_, index) => ({
      id: `flag-${index}`,
      name: "Flag",
      type: "boolean",
      currentValue: false,
    }));
    expect(decodeSessionConfig({ ...config, configOptions })).toBeNull();
  });

  it("charges confirmed identity against the 128-entry overlay limit", () => {
    const configOptions = Array.from({ length: 128 }, (_, index) => ({
      id: `flag-${index}`,
      name: "Flag",
      type: "boolean",
      currentValue: false,
    }));
    expect(
      decodeSessionConfig({ ...overlay(), modes: null, configOptions }),
    ).not.toBeNull();
    expect(
      decodeSessionConfig({
        ...overlay(),
        modes: null,
        configOptions,
        confirmedModel: { id: "actual" },
      }),
    ).toBeNull();
    expect(
      decodeSessionConfig({
        ...overlay(),
        modes: null,
        configOptions: configOptions.slice(0, 127),
        confirmedModel: { id: "actual" },
      }),
    ).not.toBeNull();
  });

  it("keeps only bounded confirmed model identity, never selection options or opaque metadata", () => {
    const decoded = decodeSessionConfig({
      ...overlay(),
      confirmedModel: {
        id: "actual",
        name: "Actual model",
        opaque: { secret: true },
      },
    });
    expect(decoded?.confirmedModel).toEqual({
      id: "actual",
      name: "Actual model",
    });
    expect(decoded?.configOptions).toEqual([select]);
    expect(
      decodeSessionConfig({ ...overlay(), confirmedModel: { id: "actual" } })
        ?.confirmedModel,
    ).toEqual({ id: "actual" });
  });

  it.each([
    null,
    { id: "" },
    { id: "x".repeat(513) },
    { id: "actual", name: "" },
    { id: "actual", name: "x".repeat(513) },
    { id: "actual", name: 1 },
  ])("rejects invalid confirmed model identity %j", (confirmedModel) => {
    expect(decodeSessionConfig({ ...overlay(), confirmedModel })).toBeNull();
  });

  it("retains v1 modes and thought_level without conflating them", () => {
    expect(decodeSessionConfig(overlay())).toEqual(overlay());
  });
  it("accepts negotiated booleans, grouped select options and removal tombstones", () => {
    const value = {
      ...overlay(),
      configOptions: [
        {
          ...select,
          options: [{ group: "effort", name: "Effort", options: [choice] }],
        },
        { id: "toggle", name: "Toggle", type: "boolean", currentValue: false },
      ],
    };
    expect(decodeSessionConfig(value)).toEqual(value);
    const tombstone = {
      ...overlay(),
      configRevision: "2",
      removed: true,
      modes: null,
      configOptions: [],
    };
    expect(decodeSessionConfig(tombstone)).toEqual(tombstone);
  });
  it("retains bounded unknown categories and discards metadata", () => {
    const value = {
      ...overlay(),
      configOptions: [
        { ...select, category: "vendor_custom", _meta: { hidden: true } },
      ],
    };
    expect(decodeSessionConfig(value)?.configOptions[0]).toEqual({
      ...select,
      category: "vendor_custom",
    });
  });
  it("keeps neighbouring options when one option advertises an unknown type", () => {
    const value = {
      ...overlay(),
      configOptions: [
        {
          id: "free-text",
          name: "Free text",
          type: "text",
          currentValue: "a",
          placeholder: "type here",
        },
        select,
      ],
    };
    const decoded = decodeSessionConfig(value);
    expect(decoded?.modes).toEqual(overlay().modes);
    expect(decoded?.configOptions).toEqual([
      { id: "free-text", name: "Free text", type: "unsupported", kind: "text" },
      select,
    ]);
  });
  it("demotes a boolean option without a negotiated boolean value", () => {
    const value = {
      ...overlay(),
      configOptions: [
        { id: "toggle", name: "Toggle", type: "boolean", currentValue: "yes" },
      ],
    };
    expect(decodeSessionConfig(value)?.configOptions).toEqual([
      { id: "toggle", name: "Toggle", type: "unsupported", kind: "boolean" },
    ]);
  });
  it("reports a missing option type as an unsupported option with an empty kind", () => {
    const value = {
      ...overlay(),
      configOptions: [{ id: "bare", name: "Bare" }],
    };
    expect(decodeSessionConfig(value)?.configOptions).toEqual([
      { id: "bare", name: "Bare", type: "unsupported", kind: "" },
    ]);
  });
  it("carries the original kind when the wire tags an option as unsupported", () => {
    const value = {
      ...overlay(),
      configOptions: [
        {
          id: "toggle",
          name: "Toggle",
          type: "unsupported",
          kind: "boolean",
          currentValue: true,
        },
      ],
    };
    expect(decodeSessionConfig(value)?.configOptions).toEqual([
      { id: "toggle", name: "Toggle", type: "unsupported", kind: "boolean" },
    ]);
  });
  it("still discards the overlay when an option omits its base identity", () => {
    const value = {
      ...overlay(),
      configOptions: [{ name: "Nameless", type: "text" }, select],
    };
    expect(decodeSessionConfig(value)).toBeNull();
  });
  it("still discards the overlay when a select advertises an unreachable current value", () => {
    const value = {
      ...overlay(),
      configOptions: [{ ...select, currentValue: "absent" }],
    };
    expect(decodeSessionConfig(value)).toBeNull();
  });
  it.each([
    { configRevision: "0" },
    { configRevision: "01" },
    { runtimeEpoch: "-1" },
    { removed: false },
    { removed: true },
    {
      modes: {
        currentModeId: "missing",
        availableModes: [{ id: "ask", name: "Ask" }],
      },
    },
    {
      modes: {
        currentModeId: "ask",
        availableModes: [
          { id: "ask", name: "Ask" },
          { id: "ask", name: "Again" },
        ],
      },
    },
    { configOptions: [select, select] },
    { configOptions: [{ ...select, currentValue: "missing" }] },
    { configOptions: [{ ...select, options: [choice, choice] }] },
    {
      configOptions: [
        {
          ...select,
          options: [choice, { group: "g", name: "Group", options: [choice] }],
        },
      ],
    },
    {
      configOptions: [
        {
          ...select,
          options: [
            { group: "g", name: "Group", options: [choice] },
            {
              group: "g",
              name: "Again",
              options: [{ value: "low", name: "Low" }],
            },
          ],
        },
      ],
    },
    {
      configOptions: Array.from({ length: 129 }, (_, i) => ({
        ...select,
        id: `effort-${i}`,
      })),
    },
    { configOptions: [{ ...select, description: "x".repeat(131073) }] },
  ])("rejects malformed or over-budget config %j", (patch) => {
    expect(decodeSessionConfig({ ...overlay(), ...patch })).toBeNull();
  });
  it("decodes a separate exact-owner wake hint", () => {
    const notice = {
      schemaVersion: 1,
      sageProjectId: "project-a",
      sageChatId: "chat-a",
      sageAgentSessionId: "session-a",
      runtimeEpoch: "2",
      configRevision: "3",
    };
    expect(decodeSessionConfigNotice(notice)).toEqual(notice);
    expect(
      decodeSessionConfigNotice({ ...notice, configRevision: "0" }),
    ).toBeNull();
  });
});
