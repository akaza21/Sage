import {
  cleanup,
  fireEvent,
  render,
  screen,
  within,
} from "@testing-library/react";
import { afterEach, expect, it } from "vitest";
import { ComposerLocationStrip } from "./ComposerLocationStrip";

afterEach(cleanup);

it("offers checked local work and announces remote work as unavailable", async () => {
  render(
    <ComposerLocationStrip projectName="Choose project" disabled={false} />,
  );
  const trigger = screen.getByRole("button", { name: "Work in" });
  fireEvent.click(trigger);
  const menu = await screen.findByRole("listbox", { name: "Work in" });
  expect(
    within(menu).getByRole("option", { name: "This computer" }),
  ).toHaveAttribute("aria-selected", "true");
  const remote = within(menu).getByRole("option", { name: /Remote/ });
  expect(remote).toBeDisabled();
  expect(remote).toHaveAttribute("aria-disabled", "true");
  expect(remote).toHaveAccessibleDescription("Not set up yet");
  fireEvent.click(remote);
  expect(menu).toBeVisible();
  fireEvent.click(within(menu).getByRole("option", { name: "This computer" }));
  expect(screen.queryByRole("listbox", { name: "Work in" })).toBeNull();
  expect(trigger).toHaveFocus();
  expect(trigger).toHaveTextContent("This computer");
});
