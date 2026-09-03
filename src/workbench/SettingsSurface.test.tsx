import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";

import { useEditorAppearanceStore } from "../app/editorAppearanceStore";
import { SettingsSurface } from "./SettingsSurface";

const initial = useEditorAppearanceStore.getState();

afterEach(() => {
  cleanup();
  useEditorAppearanceStore.setState(initial, true);
});

describe("SettingsSurface", () => {
  it("explains, changes, resets, and filters the honest Editor Appearance settings", () => {
    render(<SettingsSurface />);

    expect(
      screen.getByRole("heading", { name: "Editor Appearance" }),
    ).toBeInTheDocument();
    expect(screen.getByText("Default: 13px")).toBeInTheDocument();
    expect(screen.getByText("Default: 2 spaces")).toBeInTheDocument();
    expect(screen.getByText("Default: Off")).toBeInTheDocument();
    const reset = screen.getByRole("button", {
      name: "Reset Editor Appearance",
    });
    expect(reset).toBeDisabled();
    fireEvent.change(screen.getByLabelText("Font size"), {
      target: { value: "16" },
    });
    fireEvent.change(screen.getByLabelText("Tab size"), {
      target: { value: "4" },
    });
    fireEvent.click(screen.getByLabelText("Word wrap"));
    expect(useEditorAppearanceStore.getState()).toMatchObject({
      fontSize: 16,
      tabSize: 4,
      wordWrap: true,
    });
    expect(reset).toBeEnabled();
    fireEvent.click(reset);
    expect(useEditorAppearanceStore.getState()).toMatchObject({
      fontSize: 13,
      tabSize: 2,
      wordWrap: false,
    });
    expect(reset).toBeDisabled();
    fireEvent.change(screen.getByRole("searchbox"), {
      target: { value: "font" },
    });
    expect(screen.getByLabelText("Font size")).toBeInTheDocument();
    fireEvent.change(screen.getByRole("searchbox"), {
      target: { value: "reset" },
    });
    expect(
      screen.getByRole("button", { name: "Reset Editor Appearance" }),
    ).toBeInTheDocument();
    fireEvent.change(screen.getByRole("searchbox"), {
      target: { value: "account" },
    });
    expect(screen.getByRole("status")).toHaveTextContent(
      "No matching settings.",
    );
  });
});
