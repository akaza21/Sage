import { expect, it } from "vitest";
import type { AcpNativeCommandControl } from "../../domain/acpCommandPlacement";
import { interceptTerminalCommand } from "./interceptTerminalCommand";

const curated = [
  { name: "hooks", aliases: [] },
  { name: "permissions", aliases: ["allowed-tools"], control: "accessMenu" },
] as const;

it.each<
  [
    string,
    { name: string; aliases?: string[]; source?: string }[],
    AcpNativeCommandControl[],
    ReturnType<typeof interceptTerminalCommand>,
  ]
>([
  ["hooks", [{ name: "hooks" }], [], { kind: "send" }],
  [
    "allowed-tools",
    [],
    ["accessMenu"],
    { kind: "control", control: "accessMenu" },
  ],
  ["permissions", [], [], { kind: "terminal", name: "permissions" }],
  ["hooks", [], [], { kind: "terminal", name: "hooks" }],
  ["unknown", [], [], { kind: "send" }],
  ["hooks", [{ name: "hooks", source: "skill" }], [], { kind: "send" }],
  ["hooks", [{ name: "other", aliases: ["hooks"] }], [], { kind: "send" }],
])(
  "routes /%s by reported authority, curation and support",
  (input, reported, controls, expected) => {
    expect(
      interceptTerminalCommand(input, reported, curated, controls),
    ).toEqual(expected);
  },
);
