import { act, cleanup, render, screen } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import { snapshotPage } from "../../../app/agentDiff.fixture";
import { NavigationPart } from "../lazyParts";
import { TurnFileActivity } from "./TurnFileActivity";

vi.mock("../ChatNavigation", () => {
  throw new Error("Navigation could not load");
});

afterEach(cleanup);

it("keeps the incomplete-workspace warning visible if navigation cannot load", async () => {
  const page = snapshotPage();
  page.turns[0].items = [];
  render(
    <TurnFileActivity page={page} turn={page.turns[0]} workspaceIncomplete />,
  );
  await act(async () => {
    await expect(NavigationPart.preload()).rejects.toMatchObject({
      cause: { message: "Navigation could not load" },
    });
  });
  expect(screen.getByRole("status")).toHaveTextContent(
    "Workspace tracking is incomplete.",
  );
  expect(screen.queryByRole("region", { name: "Edited files" })).toBeNull();
});
