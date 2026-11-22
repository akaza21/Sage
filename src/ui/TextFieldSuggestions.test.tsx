import { createRef } from "react";
import { cleanup, render, screen, within } from "@testing-library/react";
import { afterEach, expect, it } from "vitest";
import { TextFieldSuggestions } from "./TextFieldSuggestions";

afterEach(cleanup);
it("keeps the source label visible and named for a single plugin group", () => {
  render(
    <TextFieldSuggestions
      anchorRef={createRef()}
      ariaLabel="Commands"
      activeId={null}
      listboxId="commands"
      state={{ kind: "ready" }}
      onActivate={() => {}}
      onDismiss={() => {}}
      onPick={() => {}}
      items={[
        {
          id: "audit",
          label: "/audit",
          accessibleLabel: "Audit",
          group: "Plugins · Inspector",
        },
      ]}
    />,
  );
  const group = screen.getByRole("group", { name: "Plugins · Inspector" });
  expect(within(group).getByText("Plugins · Inspector")).toBeVisible();
  expect(within(group).getByRole("option", { name: "Audit" })).toBeVisible();
});
