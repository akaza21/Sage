import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";

import { IconButton } from "./IconButton";

afterEach(cleanup);

describe("IconButton", () => {
  it("leaves its name to the shared tooltip instead of a native title", () => {
    render(<IconButton aria-label="Attach files" />);
    const button = screen.getByRole("button", { name: "Attach files" });
    expect(button).toHaveClass("icon-button");
    expect(button).not.toHaveAttribute("title");
  });

  it("gives a disabled button a native title, which WebKit still shows", () => {
    render(<IconButton aria-label="New terminal" disabled />);
    expect(
      screen.getByRole("button", { name: "New terminal" }),
    ).toHaveAttribute("title", "New terminal");
  });

  it("keeps an explicit title as a native title", () => {
    render(
      <IconButton aria-label="Composer options" title="No options" disabled />,
    );
    expect(
      screen.getByRole("button", { name: "Composer options" }),
    ).toHaveAttribute("title", "No options");
  });
});
