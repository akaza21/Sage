import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, expect, it } from "vitest";
import { snapshotPage } from "../../../app/agentDiff.fixture";
import { TurnEditSummary } from "./TurnEditSummary";
afterEach(cleanup);
it("hides the chat file summary when incomplete capture has no known edits", () => {
  const page = snapshotPage();
  page.turns[0].items = [];
  render(
    <TurnEditSummary
      page={page}
      rootLabels={{}}
      countIncomplete
      workspaceChanges={[]}
      workspaceIncomplete
    />,
  );
  expect(
    screen.queryByRole("region", { name: "Files edited in this chat" }),
  ).toBeNull();
  expect(screen.queryByText(/Showing 0 changed files/)).toBeNull();
});
