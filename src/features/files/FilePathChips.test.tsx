import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";

import { FileEditorHeader } from "./FileEditorHeader";

afterEach(cleanup);

describe("FilePathChips", () => {
  it("renders a bounded chip trail for the open editor path", () => {
    render(
      <FileEditorHeader
        lineEnding="lf"
        diskByteLen={0}
        persisting={false}
        dirty={false}
        saving={false}
        transitioning={false}
        path="src/train.py"
      />,
    );
    expect(
      screen.getByRole("navigation", { name: "File path" }),
    ).toHaveTextContent("srctrain.py");
    expect(screen.getByText("train.py")).toBeInTheDocument();
  });

  it("includes the Project name in an Agent file breadcrumb", () => {
    render(
      <FileEditorHeader
        lineEnding="lf"
        diskByteLen={0}
        persisting={false}
        dirty={false}
        saving={false}
        transitioning={false}
        path="docs/readme.md"
        projectName="Sage"
      />,
    );
    expect(
      screen.getByRole("navigation", { name: "File path" }),
    ).toHaveTextContent("Sagedocsreadme.md");
  });

  it("keeps the first directory and marks omitted middle segments", () => {
    render(
      <FileEditorHeader
        lineEnding="lf"
        diskByteLen={0}
        persisting={false}
        dirty={false}
        saving={false}
        transitioning={false}
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
