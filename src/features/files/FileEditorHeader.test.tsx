import { render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import { FileEditorHeader } from "./FileEditorHeader";

const requiredProps = {
  path: "src/main.ts",
  lineEnding: "lf",
  diskByteLen: 128,
  persisting: false,
  dirty: false,
  saving: false,
  transitioning: false,
};

describe("FileEditorHeader", () => {
  it("keeps disk actions on standard editor commands instead of header chrome", () => {
    render(<FileEditorHeader {...requiredProps} dirty />);

    expect(screen.queryByRole("button", { name: "Save" })).toBeNull();
    expect(screen.queryByRole("button", { name: "Discard" })).toBeNull();
    expect(
      screen.getByRole("status", {
        name: "Unsaved changes. Press Control or Command plus S to save.",
      }),
    ).toHaveTextContent("Unsaved");
  });

  it("announces the saving and recovery-snapshot states accurately", () => {
    const view = render(<FileEditorHeader {...requiredProps} saving />);
    expect(
      screen.getByRole("status", { name: "Saving file" }),
    ).toHaveTextContent("Saving");

    view.rerender(
      <FileEditorHeader {...requiredProps} dirty persisting saving={false} />,
    );
    expect(
      screen.getByRole("status", { name: "Protecting recovery copy" }),
    ).toHaveTextContent("Protecting changes");
  });

  it("exposes Preview as a real pressed toggle", () => {
    const onTogglePreview = vi.fn();
    const view = render(
      <FileEditorHeader
        {...requiredProps}
        canPreview
        onTogglePreview={onTogglePreview}
      />,
    );

    const preview = screen.getByRole("button", { name: "Preview file" });
    expect(preview).toHaveAttribute("aria-pressed", "false");

    view.rerender(
      <FileEditorHeader
        {...requiredProps}
        canPreview
        previewing
        onTogglePreview={onTogglePreview}
      />,
    );
    expect(
      screen.getByRole("button", { name: "Return to editor" }),
    ).toHaveAttribute("aria-pressed", "true");
  });
});
