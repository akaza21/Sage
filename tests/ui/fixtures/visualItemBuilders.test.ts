import { describe, expect, it } from "vitest";

import { TOOL_KIND_PRESENTATIONS, toolItem } from "./visualItemBuilders";
import { safeToolPresentation } from "../../../src/services/acpDesktopResponsePayloads";

const input = { turn: 1, ordinal: 1 } as never;

describe("visual tool fixture builder", () => {
  it("covers every tool kind in the projection contract", () => {
    expect(Object.keys(TOOL_KIND_PRESENTATIONS).sort()).toEqual([
      "delete",
      "edit",
      "execute",
      "fetch",
      "mcp",
      "move",
      "other",
      "read",
      "search",
      "subagent",
      "switchMode",
      "think",
    ]);
  });

  it("builds a card for each kind with that kind on the presentation", () => {
    for (const [kind, presentation] of Object.entries(
      TOOL_KIND_PRESENTATIONS,
    )) {
      const built = toolItem(input, "completed", presentation);
      expect(built.kind).toBe("toolCall");
      expect(built.payload.presentation.kind).toBe(kind);
      expect(built.payload.presentation.label.length).toBeGreaterThan(0);
    }
  });

  it("keeps the search presentation as the default so existing scenarios are unchanged", () => {
    const built = toolItem(input, "completed");
    expect(built.payload.presentation.kind).toBe("search");
    expect(built.payload.presentation.label).toBe("Search workspace");
  });

  it("leaves a running tool without a terminal timestamp", () => {
    const running = toolItem(
      input,
      "inProgress",
      TOOL_KIND_PRESENTATIONS.execute,
    );
    expect(running.terminalAt).toBeNull();
    expect(running.toolState).toBe("inProgress");
  });

  it("offers only presentations production would accept off the wire", () => {
    for (const [kind, presentation] of Object.entries(
      TOOL_KIND_PRESENTATIONS,
    )) {
      expect(
        safeToolPresentation(presentation),
        `the ${kind} fixture presentation survives the production parser`,
      ).not.toBeNull();
    }
  });
});
