import { expect, it } from "vitest";
import { decodeBackgroundTerminals } from "./acpBackgroundTerminals";

it("decodes bounded background terminals and rejects anything else", () => {
  const terminals = [
    {
      handle: "7",
      command: "npm run dev",
      cwd: {
        kind: "resource",
        resource: { rootId: "root-1", relativePath: "web" },
      },
    },
    { handle: "8", command: "sleep 60", cwd: null },
  ];
  expect(decodeBackgroundTerminals(terminals)).toEqual(terminals);
  expect(decodeBackgroundTerminals([])).toEqual([]);

  const [first] = terminals;
  for (const bad of [
    { ...first, handle: "" },
    { ...first, handle: "h".repeat(129) },
    { ...first, command: 4 },
    { ...first, command: "x".repeat(4_097) },
    { ...first, cwd: { kind: "resource" } },
  ])
    expect(decodeBackgroundTerminals([bad])).toBeNull();
  expect(decodeBackgroundTerminals({ terminals })).toBeNull();
  expect(
    decodeBackgroundTerminals(Array.from({ length: 33 }, () => first)),
  ).toBeNull();
});
