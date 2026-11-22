import { describe, expect, it } from "vitest";
import { decodeCommandsNotice } from "./acpCommandsDesktop";
import { decodeCommandList } from "./acpCommandCatalogDesktop";
describe("native command list contract", () => {
  it("preserves advertised hints and empty lists", () => {
    expect(decodeCommandList({ revision: 0, commands: [] })).toEqual({
      revision: 0,
      pending: false,
      omitted: 0,
      commands: [],
    });
    const value = {
      revision: 1,
      pending: true,
      omitted: 88,
      commands: [
        {
          name: "plan",
          description: "Plan",
          inputHint: "task",
          source: "skill",
          sourceLabel: "user",
        },
      ],
    };
    expect(decodeCommandList(value)).toEqual(value);
    const status = { ...value.commands[0]!, name: "status", panel: "status" };
    expect(
      decodeCommandList({ revision: 2, commands: [status] })?.commands[0]
        ?.panel,
    ).toBe("status");
    const usage = { ...value.commands[0]!, name: "usage", aliases: ["cost"] };
    expect(
      decodeCommandList({ revision: 3, commands: [usage] })?.commands[0]
        ?.aliases,
    ).toEqual(["cost"]);
    const placed = {
      ...usage,
      placement: { section: "usage", control: "usagePanel" },
    };
    expect(
      decodeCommandList({ revision: 4, commands: [placed] })?.commands,
    ).toEqual([placed]);
  });
  it("rejects duplicate names, malformed entries, unsafe revisions and oversized lists", () => {
    const command = {
      name: "plan",
      description: "Plan",
      inputHint: null,
      source: "harness",
    };
    for (const value of [
      null,
      { revision: 1, commands: [], pending: "yes" },
      { revision: 1, commands: [], omitted: -1 },
      { revision: 1, commands: [], omitted: 2 ** 32 },
      { revision: 1, commands: [], omitted: 1.5 },
      { revision: 1, commands: [{ ...command, sourceLabel: "x".repeat(129) }] },
      { revision: -1, commands: [] },
      { revision: 2 ** 53, commands: [] },
      { revision: 1, commands: [command, command] },
      { revision: 1, commands: [{ ...command, name: "/plan" }] },
      { revision: 1, commands: [{ ...command, source: "invented" }] },
      { revision: 1, commands: [{ ...command, source: undefined }] },
      { revision: 1, commands: [{ ...command, panel: "settings" }] },
      { revision: 1, commands: [{ ...command, panel: null }] },
      {
        revision: 1,
        commands: [{ ...command, extra: { retained: "unexpected" } }],
      },
      {
        revision: 1,
        commands: [{ ...command, placement: { section: "invented" } }],
      },
      {
        revision: 1,
        commands: [
          {
            ...command,
            placement: { section: "session", control: "invented" },
          },
        ],
      },
      {
        revision: 1,
        commands: [
          { ...command, placement: { section: "session", extra: true } },
        ],
      },
      { revision: 1, commands: [{ ...command, aliases: ["bad name"] }] },
      { revision: 1, commands: [{ ...command, aliases: "cost" }] },
      { revision: 1, commands: [{ ...command, aliases: null }] },
      {
        revision: 1,
        commands: [{ ...command, aliases: Array(9).fill("cost") }],
      },
      { revision: 1, commands: Array(513).fill(command) },
      {
        revision: 1,
        commands: [{ ...command, description: "x".repeat(1025) }],
      },
    ])
      expect(decodeCommandList(value)).toBeNull();
  });
  it("admits only a well-formed change notice", () => {
    const notice = {
      schemaVersion: 1,
      sageProjectId: "project",
      sageChatId: "chat",
      sageAgentSessionId: "session",
      runtimeEpoch: "1",
      revision: 1,
    };
    const { schemaVersion: _version, ...decoded } = notice;
    expect(decodeCommandsNotice(notice)).toEqual(decoded);
    for (const value of [
      { ...notice, schemaVersion: 2 },
      { ...notice, sageChatId: "" },
      { ...notice, sageChatId: "invalid\0chat" },
      { ...notice, runtimeEpoch: 1 },
      { ...notice, runtimeEpoch: "0" },
      { ...notice, revision: 0 },
    ])
      expect(decodeCommandsNotice(value)).toBeNull();
  });
  it("counts every advertised string once at the exact catalog byte boundary", () => {
    const commands = Array.from({ length: 256 }, (_, index) => ({
      name: `c${String(index).padStart(3, "0")}`,
      description: "d".repeat(1000),
      inputHint: "h".repeat(8),
      source: "builtin",
      sourceLabel: "core",
      aliases: [`alias${String(index).padStart(3, "0")}`],
    }));
    expect(decodeCommandList({ revision: 1, commands })?.commands).toHaveLength(
      256,
    );
    commands[255]!.inputHint += "h";
    expect(decodeCommandList({ revision: 2, commands })).toBeNull();
  });
});
