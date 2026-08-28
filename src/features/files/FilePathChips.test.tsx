import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";

import { FilePathChips } from "./FilePathChips";

afterEach(cleanup);

describe("FilePathChips", () => {
  it("renders a bounded chip trail for the open editor path", () => {
    render(<FilePathChips path="src/train.py" />);
    expect(
      screen.getByRole("navigation", { name: "File path" }),
    ).toHaveTextContent("srctrain.py");
    expect(screen.getByText("train.py")).toBeInTheDocument();
  });
});
