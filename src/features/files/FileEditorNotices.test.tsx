import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";

import { FileEditorNotices } from "./FileEditorNotices";

afterEach(cleanup);

describe("FileEditorNotices", () => {
  it("lets an external-change conflict own the error surface", () => {
    render(
      <FileEditorNotices
        conflict={{
          kind: "external-change",
          message: "This file changed outside Sage.",
        }}
        message="invalid request: snapshot base no longer matches disk"
      />,
    );

    expect(screen.getAllByRole("alert")).toHaveLength(1);
    expect(screen.getByText(/External change detected/)).toBeVisible();
    expect(screen.queryByText(/invalid request/)).toBeNull();
  });

  it("keeps a standalone editor error visible", () => {
    render(<FileEditorNotices conflict={null} message="Save failed." />);

    expect(screen.getByRole("alert")).toHaveTextContent("Save failed.");
  });
});
