import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, expect, it } from "vitest";

import type { AgentArtifactOwner } from "../domain/agentArtifact";
import { SubagentResourceRows } from "./SubagentResourceRows";

afterEach(cleanup);

const agents = [
  {
    id: "spawn:child",
    label: "Reviewer",
    status: "working" as const,
    target: {
      owner: {
        projectId: "project-a",
        chatId: "chat:a",
        sessionId: "session-1",
      } as AgentArtifactOwner,
      itemId: "spawn",
      agentKey: "child",
    },
  },
];

it("focuses the first dialog control in document order when lazy rows arrive", () => {
  const { rerender } = render(
    <div role="dialog" tabIndex={-1}>
      <button>Sources</button>
    </div>,
  );
  screen.getByRole("dialog").focus();
  rerender(
    <div role="dialog" tabIndex={-1}>
      <button>Sources</button>
      <SubagentResourceRows agents={agents} onSelect={() => {}} />
    </div>,
  );
  expect(screen.getByRole("button", { name: "Sources" })).toHaveFocus();
});

it("does not move focus from Sources when lazy rows arrive", () => {
  const { rerender } = render(
    <div role="dialog" tabIndex={-1}>
      <button>Sources</button>
    </div>,
  );
  screen.getByRole("button", { name: "Sources" }).focus();
  rerender(
    <div role="dialog" tabIndex={-1}>
      <button>Sources</button>
      <SubagentResourceRows agents={agents} onSelect={() => {}} />
    </div>,
  );
  expect(screen.getByRole("button", { name: "Sources" })).toHaveFocus();
});
