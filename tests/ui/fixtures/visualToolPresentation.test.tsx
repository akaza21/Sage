import { afterEach, describe, expect, it } from "vitest";
import { cleanup, render } from "@testing-library/react";

import type { AcpItem } from "../../../src/domain/acp";
import { ToolItem } from "../../../src/features/chat/acp/ToolItem";

import { VISUAL_SCENARIOS } from "./visualScenarioCatalog";
import { buildVisualScenario } from "./visualScenarioShell";

type ToolProjectionItem = Extract<AcpItem, { kind: "toolCall" }>;

function fixtureToolCalls(): {
  scenario: string;
  item: ToolProjectionItem;
}[] {
  return VISUAL_SCENARIOS.flatMap((scenario) =>
    buildVisualScenario(scenario.name).pages.flatMap((page) =>
      page.turns.flatMap((turn) =>
        turn.items
          .filter(
            (item): item is ToolProjectionItem => item.kind === "toolCall",
          )
          .map((item) => ({ scenario: scenario.name, item })),
      ),
    ),
  );
}

afterEach(cleanup);

describe("visual fixture tool presentations", () => {
  it("renders every fixture tool call through a production tool kind", () => {
    const calls = fixtureToolCalls();
    expect(
      calls.length,
      "the visual fixtures still carry tool calls to guard",
    ).toBeGreaterThan(5);
    for (const { scenario, item } of calls) {
      const kind = item.payload.presentation.kind;
      const view = render(<ToolItem item={item} />);
      expect(
        view.container.querySelector(`[data-tool-kind-icon="${kind}"]`),
        `${scenario} renders the ${kind} tool card with a known kind icon`,
      ).not.toBeNull();
      view.unmount();
    }
  });
});
