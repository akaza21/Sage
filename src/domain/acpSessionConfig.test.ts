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
  it("retains v1 modes and thought_level without conflating them", () => {
    expect(decodeSessionConfig(overlay())).toEqual(overlay());
  });
  it("accepts boolean, homogeneous grouped select options and removal tombstones", () => {
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
    { configOptions: [{ ...select, type: "text" }] },
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
