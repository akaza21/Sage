import { act, cleanup, fireEvent, render } from "@testing-library/react";
import { useState } from "react";
import { afterEach, describe, expect, it } from "vitest";

import type { AcpChatPage, AcpItem, TurnProjection } from "../../../domain/acp";
import { settleLazyModules } from "../../../test/settleLazyModules";
import { AcpChatProjection } from "./AcpChatProjection";
import { page } from "./acpProjection.fixture";
import parity from "./harnessRenderParity.fixture.json";

// The shared fixture the Rust parity test drives every binding against
// (docs/harness-render-contract.md): one canonical projection per scenario,
// plus each harness's known differences from it.

type Projection = { turnState: string; items: Record<string, unknown>[] };

const HARNESSES = ["claude", "codex", "pi", "acp"] as const;

afterEach(cleanup);

/** Restores the store-assigned identity the parity fixture strips. */
function chatPage({ turnState, items }: Projection): AcpChatPage {
  const base = page(turnState as TurnProjection["state"]);
  base.sessions = [{ id: "session-1", ordinal: 0 } as never];
  base.turns[0].items = items.map((item, ordinal) => {
    const payload = structuredClone(item.payload) as Record<string, unknown>;
    if (item.kind === "fileChangeDiff") {
      for (const change of payload.changes as Record<string, unknown>[]) {
        const available = change.pairState === "available";
        change.snapshotRevision = available ? "1" : "0";
        change.contentDigest = available ? "0".repeat(64) : null;
      }
    }
    return {
      ...item,
      payload,
      id: `item-${ordinal}`,
      projectId: "project-1",
      chatId: "chat-1",
      sessionId: "session-1",
      turnId: "turn-1",
      runtimeEpoch: "2",
      ordinal,
      origin: "primary",
      activityId: null,
      revision: "1",
      createdAt: 1_000,
      updatedAt: 2_000,
      terminalAt: 2_000,
    } as unknown as AcpItem;
  });
  return base;
}

function Chat({ projection }: { projection: AcpChatPage }) {
  const [container, setContainer] = useState<HTMLDivElement | null>(null);
  return (
    <>
      <AcpChatProjection
        page={projection}
        active
        editSummaryContainer={container}
      />
      <div ref={setContainer} data-edit-summary="" />
    </>
  );
}

const implicitRoles: Record<string, string> = {
  A: "link",
  ARTICLE: "article",
  BUTTON: "button",
  LI: "listitem",
  OL: "list",
  SUMMARY: "summary",
  UL: "list",
};

/** Roles, accessible names, link targets and text in document order. */
function outline(root: Element, lines: string[] = []): string[] {
  for (const node of root.childNodes) {
    if (node.nodeType === Node.TEXT_NODE) {
      const text = node.textContent?.replace(/\s+/g, " ").trim();
      if (text) lines.push(`  ${text}`);
      continue;
    }
    if (!(node instanceof Element)) continue;
    // Clock text depends on the runner's time zone.
    if (node.matches("[hidden],[aria-hidden='true'],time")) continue;
    const role =
      node.getAttribute("role") ??
      implicitRoles[node.tagName] ??
      (node.matches(".markdown-web-link") ? "web link" : null);
    if (role) {
      const name = node.getAttribute("aria-label");
      const target = node.getAttribute("href") ?? node.getAttribute("title");
      lines.push(
        [role, name && `"${name}"`, role.endsWith("link") && `-> ${target}`]
          .filter(Boolean)
          .join(" "),
      );
    }
    outline(node, lines);
  }
  return lines;
}

/** Renders a projection with every disclosure opened, as a reader would. */
async function rendered(projection: Projection): Promise<string[]> {
  const { container } = render(<Chat projection={chatPage(projection)} />);
  await settleLazyModules();
  for (let pass = 0; pass < 4; pass += 1) {
    const closed = container.querySelectorAll<HTMLDetailsElement>(
      "details:not([open])",
    );
    if (closed.length === 0) break;
    for (const disclosure of closed) {
      await act(async () => {
        disclosure.open = true;
        fireEvent(disclosure, new Event("toggle"));
      });
    }
    await settleLazyModules();
  }
  const roots = container.querySelectorAll(
    "[data-turn-id],[data-edit-summary]",
  );
  const lines = [...roots].flatMap((root) => outline(root));
  cleanup();
  return lines;
}

/** The canonical projection with a harness's known differences in place. */
function harnessProjection(
  scenario: string,
  harness: string,
  canonical: Projection,
): Projection {
  const gap = parity.gaps.find(
    (entry) => entry.scenario === scenario && entry.harness === harness,
  );
  const projection = structuredClone(canonical) as Record<string, unknown>;
  for (const [pointer, value] of Object.entries(gap?.differs ?? {})) {
    const path = pointer.split("/").slice(1);
    const field = path.pop()!;
    const parent = path.reduce<Record<string, unknown>>(
      (node, key) => node[key] as Record<string, unknown>,
      projection,
    );
    parent[field] = value;
  }
  return projection as Projection;
}

const scenarios = Object.entries(parity.scenarios) as [
  string,
  { title: string; projection: Projection; outline: string[] },
][];

describe("harness render parity", () => {
  it.each(scenarios)(
    "%s renders through the one chat view",
    async (_name, scenario) => {
      expect(await rendered(scenario.projection)).toEqual(scenario.outline);
    },
  );

  it.each(scenarios)(
    "%s looks the same for every harness except its visible gaps",
    async (name, scenario) => {
      for (const harness of HARNESSES) {
        const applicable = !parity.notApplicable.some(
          (entry) => entry.scenario === name && entry.harness === harness,
        );
        if (!applicable) continue;
        const gap = parity.gaps.find(
          (entry) => entry.scenario === name && entry.harness === harness,
        );
        const lines = await rendered(
          harnessProjection(name, harness, scenario.projection),
        );
        const same = JSON.stringify(lines) === JSON.stringify(scenario.outline);
        expect(
          { harness, same },
          "a gap's visible flag must match what the reader sees",
        ).toEqual({ harness, same: !gap?.visible });
      }
    },
  );
});
