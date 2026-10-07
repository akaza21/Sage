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

  it("includes the Project name in an Agent file breadcrumb", () => {
    render(<FilePathChips path="docs/readme.md" projectName="Sage" />);
    expect(
      screen.getByRole("navigation", { name: "File path" }),
    ).toHaveTextContent("Sagedocsreadme.md");
  });

  it("keeps the first directory and marks omitted middle segments", () => {
    render(
      <FilePathChips
        path="first/second/third/fourth/fifth/readme.md"
        projectName="Sage"
      />,
    );
    const breadcrumb = screen.getByRole("navigation", { name: "File path" });
    expect(breadcrumb).toHaveTextContent("Sagefirst…fourthfifthreadme.md");
    expect(breadcrumb.querySelectorAll(".file-breadcrumb__item")).toHaveLength(
      6,
    );
  });
});
