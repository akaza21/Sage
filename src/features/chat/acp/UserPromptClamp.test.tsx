import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import { UserPromptClamp } from "./UserPromptClamp";

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
});

function layout(content: number, frame: number) {
  vi.spyOn(HTMLElement.prototype, "scrollHeight", "get").mockReturnValue(
    content,
  );
  vi.spyOn(HTMLElement.prototype, "clientHeight", "get").mockReturnValue(
    frame,
  );
}

it("clamps a prompt taller than six lines and expands it on request", () => {
  layout(400, 120);
  const { container } = render(
    <UserPromptClamp>
      <p>Long prompt</p>
    </UserPromptClamp>,
  );
  const frame = container.querySelector(".acp-message__clamp")!;
  expect(frame).toHaveAttribute("data-clamped");
  const expand = screen.getByRole("button", { name: "Expand" });
  expect(expand).toHaveAttribute("aria-expanded", "false");

  fireEvent.click(expand);
  expect(frame).toHaveAttribute("data-expanded");
  expect(frame).not.toHaveAttribute("data-clamped");
  fireEvent.click(screen.getByRole("button", { name: "Collapse" }));
  expect(frame).toHaveAttribute("data-clamped");
});

it("shows no Expand for a prompt that fits", () => {
  layout(60, 120);
  const { container } = render(
    <UserPromptClamp>
      <p>Short prompt</p>
    </UserPromptClamp>,
  );
  expect(container.querySelector("[data-clamped]")).toBeNull();
  expect(screen.queryByRole("button")).toBeNull();
});
