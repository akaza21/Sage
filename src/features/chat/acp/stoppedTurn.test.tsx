import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, expect, it } from "vitest";
import { settleLazyModules } from "../../../test/settleLazyModules";
import { AcpChatProjection } from "./AcpChatProjection";
import { page, textItem } from "./acpProjection.fixture";

afterEach(cleanup);

it.each(["", " \n\t "])(
  "does not claim a retained partial answer for blank assistant text %j",
  async (text) => {
    const current = page("interrupted", { kind: "cancelledByUser" });
    current.turns[0].items = [
      {
        ...textItem(),
        lifecycle: "cancelled",
        payload: { role: "assistant", text },
      },
    ];
    render(<AcpChatProjection page={current} active />);
    await settleLazyModules();
    expect(
      screen.getByRole("status", { name: "Cancelled by you" }),
    ).toBeVisible();
    expect(screen.queryByText("Partial answer retained")).toBeNull();
    expect(document.querySelector('[data-answer-state="partial"]')).toBeNull();
  },
);

it.each([
  ["interrupted", "cancelledByUser", "Cancelled by you"],
  ["cancelled", "cancelledByUser", "Cancelled by you"],
  ["interrupted", undefined, "Interrupted"],
  ["cancelled", undefined, "Cancelled"],
] as const)(
  "shows one consistent %s notice for reason %s",
  async (state, kind, label) => {
    const current = page(state, kind ? { kind } : undefined);
    current.turns[0].items = [{ ...textItem(), lifecycle: "cancelled" }];
    render(<AcpChatProjection page={current} active />);
    await settleLazyModules();
    expect(screen.getAllByText(label)).toHaveLength(1);
    expect(screen.getByRole("status", { name: label })).toBeVisible();
    expect(screen.getByText("Partial answer retained")).toBeVisible();
    expect(
      screen.getByText("Normalized answer").closest(".acp-message"),
    ).toHaveAttribute("data-answer-state", "partial");
    if (kind === "cancelledByUser")
      expect(screen.queryByText("Interrupted")).toBeNull();
  },
);

it.each(["empty", "completed"] as const)(
  "keeps a stopped notice without inventing partial text for an %s answer",
  async (scenario) => {
    const current = page("interrupted", { kind: "cancelledByUser" });
    current.turns[0].items =
      scenario === "empty" ? [] : [{ ...textItem(), lifecycle: "completed" }];
    render(<AcpChatProjection page={current} active />);
    await settleLazyModules();
    expect(screen.getAllByText("Cancelled by you")).toHaveLength(1);
    expect(screen.queryByText("Partial answer retained")).toBeNull();
    expect(document.querySelector('[data-answer-state="partial"]')).toBeNull();
  },
);
