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
  it("filters the honest Editor Appearance settings", () => {
    render(<SettingsSurface />);

    expect(
      screen.getByRole("heading", { name: "Editor Appearance" }),
    ).toBeInTheDocument();
    fireEvent.change(screen.getByRole("searchbox"), {
      target: { value: "font" },
    });
    expect(screen.getByLabelText("Font size")).toBeInTheDocument();
    fireEvent.change(screen.getByRole("searchbox"), {
      target: { value: "account" },
    });
    expect(screen.getByRole("status")).toHaveTextContent(
      "No matching settings.",
    );
  });
});
