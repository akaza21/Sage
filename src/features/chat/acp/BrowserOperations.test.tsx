import { render, screen } from "@testing-library/react";
import { expect, it } from "vitest";

import { browserOperationFixture } from "../../../domain/browserOperation.fixture";
import { BrowserOperations } from "./BrowserOperations";

it("shows retained browser actions in their own transcript category and counts older actions", () => {
  render(
    <BrowserOperations operations={[browserOperationFixture]} collapsed={4} />,
  );
  expect(
    screen.getByRole("region", { name: "Browser actions" }),
  ).toBeInTheDocument();
  expect(screen.getByText("+4 earlier browser actions")).toBeInTheDocument();
  expect(
    screen.getByRole("group", {
      name: "Clicked element on example.com · Completed",
    }),
  ).toBeInTheDocument();
});
