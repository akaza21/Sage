import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
} from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import type { TurnProjection } from "../../domain/acp";
import { page, textItem } from "./acp/acpProjection.fixture";
import { TurnNavigator } from "./TurnNavigator";

afterEach(() => {
  cleanup();
  vi.useRealTimers();
});

function turns(count: number): TurnProjection[] {
  return Array.from({ length: count }, (_, index) => ({
    ...page("completed").turns[0]!,
    id: `turn-${index}` as never,
    ordinal: index + 1,
    items: [
      {
        ...textItem(),
        id: `prompt-${index}` as never,
        origin: "primary" as const,
        payload: { role: "user" as const, text: `Prompt  ${index + 1}` },
      },
      {
        ...textItem(),
        id: `answer-${index}` as never,
        origin: "primary" as const,
        payload: {
          role: "assistant" as const,
          text: `Answer **${index + 1}**\n\n- first point`,
        },
      },
    ],
  }));
}

function widths() {
  return screen
    .getAllByRole("button", { name: /^Go to turn/ })
    .map((button) => button.style.getPropertyValue("--w"));
}

it("labels the rail, widens like Codex, delays the preview card and jumps", async () => {
  vi.useFakeTimers();
  const jump = vi.fn();
  render(
    <TurnNavigator
      turns={turns(6)}
      visibleIds={["turn-2", "turn-3"]}
      top={40}
      height={600}
      onJump={jump}
    />,
  );
  expect(
    screen.getByRole("list", { name: "Turn navigator" }),
  ).toBeInTheDocument();
  const buttons = screen.getAllByRole("button", { name: /^Go to turn/ });
  expect(buttons).toHaveLength(6);
  expect(buttons[0]).toHaveAccessibleName("Go to turn 1: Prompt 1");
  expect(buttons.map((button) => button.dataset.visible)).toEqual([
    "false",
    "false",
    "true",
    "true",
    "false",
    "false",
  ]);
  expect(buttons[2]).toHaveAttribute("aria-current", "location");
  expect(widths()).toEqual(["6px", "6px", "6px", "6px", "6px", "6px"]);
  fireEvent.pointerEnter(buttons[2]!);
  expect(widths()).toEqual(["13px", "18px", "24px", "18px", "13px", "9px"]);
  expect(screen.queryByRole("tooltip")).not.toBeInTheDocument();
  await act(async () => vi.advanceTimersByTime(120));
  const card = screen.getByRole("tooltip");
  expect(card).toHaveTextContent("Prompt 3");
  expect(card).toHaveTextContent("Answer 3");
  expect(card).toHaveTextContent("• first point");
  expect(buttons[2]).toHaveAttribute("aria-describedby", card.id);
  fireEvent.pointerEnter(buttons[3]!);
  expect(screen.getByRole("tooltip")).toHaveTextContent("Prompt 4");
  fireEvent.click(buttons[3]!);
  expect(jump).toHaveBeenCalledWith("turn-3");
  expect(screen.getByRole("tooltip")).toBeInTheDocument();
  fireEvent.pointerLeave(buttons[3]!.closest(".turn-navigator")!);
  expect(screen.queryByRole("tooltip")).not.toBeInTheDocument();
});

it("uses one roving tab stop and arrow keys between dashes", () => {
  render(
    <TurnNavigator
      turns={turns(6)}
      visibleIds={["turn-4"]}
      top={0}
      height={600}
      onJump={() => {}}
    />,
  );
  const buttons = screen.getAllByRole("button", { name: /^Go to turn/ });
  expect(buttons.map((button) => button.tabIndex)).toEqual([
    -1, -1, -1, -1, 0, -1,
  ]);
  buttons[4]!.focus();
  fireEvent.keyDown(buttons[4]!, { key: "ArrowDown" });
  expect(buttons[5]).toHaveFocus();
  fireEvent.keyDown(buttons[5]!, { key: "ArrowDown" });
  expect(buttons[5]).toHaveFocus();
  fireEvent.keyDown(buttons[5]!, { key: "Home" });
  expect(buttons[0]).toHaveFocus();
  expect(buttons[0]!.tabIndex).toBe(0);
  fireEvent.keyDown(buttons[0]!, { key: "ArrowUp" });
  expect(buttons[0]).toHaveFocus();
  fireEvent.keyDown(buttons[0]!, { key: "End" });
  expect(buttons[5]).toHaveFocus();
});

it("caps the dashes to the rail height and hides below two turns", () => {
  const view = render(
    <TurnNavigator
      turns={turns(200)}
      visibleIds={["turn-150"]}
      top={0}
      height={900}
      onJump={() => {}}
    />,
  );
  const buttons = screen.getAllByRole("button", { name: /^Go to turn/ });
  expect(buttons).toHaveLength(60);
  expect(buttons[0]).toHaveAccessibleName(/^Go to turn 1:/);
  expect(buttons.at(-1)).toHaveAccessibleName(/^Go to turn 200:/);
  expect(
    screen.getByRole("button", { name: /^Go to turn 151:/ }),
  ).toHaveAttribute("aria-current", "location");
  view.rerender(
    <TurnNavigator
      turns={turns(200)}
      visibleIds={[]}
      top={0}
      height={180}
      onJump={() => {}}
    />,
  );
  expect(screen.getAllByRole("button", { name: /^Go to turn/ })).toHaveLength(
    20,
  );
  view.rerender(
    <TurnNavigator
      turns={turns(1)}
      visibleIds={[]}
      top={0}
      height={600}
      onJump={() => {}}
    />,
  );
  expect(
    screen.queryByRole("list", { name: "Turn navigator" }),
  ).not.toBeInTheDocument();
});
