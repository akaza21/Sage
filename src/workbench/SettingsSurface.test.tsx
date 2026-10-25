import {
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { useEditorSettingsStore } from "../app/editorSettingsStore";
import { SettingsSurface } from "./SettingsSurface";

const initial = useEditorSettingsStore.getState();

beforeEach(() => {
  useEditorSettingsStore.setState({
    persistenceClosing: false,
    persistenceReady: true,
    persistencePhase: "idle",
  });
});

afterEach(() => {
  cleanup();
  useEditorSettingsStore.setState(initial, true);
  vi.restoreAllMocks();
});

describe("SettingsSurface", () => {
  it("lists the settings pages in a nav and opens Editor first", () => {
    render(<SettingsSurface />);
    const nav = screen.getByRole("navigation", { name: "Settings pages" });
    expect(
      within(nav)
        .getAllByRole("tab")
        .map((tab) => tab.textContent),
    ).toEqual(["Editor", "Keyboard"]);
    expect(within(nav).getByRole("tab", { name: "Editor" })).toHaveAttribute(
      "aria-selected",
      "true",
    );
  });

  it("shows the real chord for each shortcut on the Keyboard page", () => {
    render(<SettingsSurface />);
    fireEvent.click(screen.getByRole("tab", { name: "Keyboard" }));
    expect(
      screen.getByRole("heading", { name: "Keyboard Shortcuts" }),
    ).toBeInTheDocument();
    const save = screen.getByText("Save file").closest("tr");
    expect(save).not.toBeNull();
    expect(within(save as HTMLElement).getByText("Ctrl+S")).toBeInTheDocument();
    expect(screen.getByText("Go to symbol").closest("tr")).toHaveTextContent(
      "F12",
    );
  });

  it("keeps the Keyboard page read-only", () => {
    render(<SettingsSurface />);
    fireEvent.click(screen.getByRole("tab", { name: "Keyboard" }));
    expect(screen.queryByRole("checkbox")).toBeNull();
    expect(screen.queryByRole("combobox")).toBeNull();
  });

  it("marks nav pages that have no match for the current query", () => {
    render(<SettingsSurface />);
    fireEvent.change(screen.getByRole("searchbox"), {
      target: { value: "go to symbol" },
    });
    const nav = screen.getByRole("navigation", { name: "Settings pages" });
    expect(within(nav).getByRole("tab", { name: "Editor" })).toHaveAttribute(
      "data-matches",
      "false",
    );
    expect(within(nav).getByRole("tab", { name: "Keyboard" })).toHaveAttribute(
      "data-matches",
      "true",
    );
  });

  it("filters the Keyboard page rows by the same query", () => {
    render(<SettingsSurface />);
    fireEvent.click(screen.getByRole("tab", { name: "Keyboard" }));
    fireEvent.change(screen.getByRole("searchbox"), {
      target: { value: "go to symbol" },
    });
    expect(screen.getByText("Go to symbol")).toBeInTheDocument();
    expect(screen.queryByText("Save file")).toBeNull();
  });

  it("explains, changes, and resets the five honest Editor Appearance settings", async () => {
    render(<SettingsSurface />);

    expect(
      screen.getByRole("heading", { name: "Editor Appearance" }),
    ).toBeInTheDocument();
    expect(screen.getByText("Default: 13px")).toBeInTheDocument();
    expect(screen.getByText("Default: 2 spaces")).toBeInTheDocument();
    expect(screen.getByText("Default: On")).toBeInTheDocument();
    expect(screen.getAllByText("Default: Off")).toHaveLength(2);
    expect(screen.getByRole("searchbox")).toHaveClass("input");
    const display = screen.getByRole("group", { name: "Text" });
    const indentation = screen.getByRole("group", { name: "Editing" });
    expect(within(display).getByLabelText("Font size")).toBeInTheDocument();
    expect(within(display).getByLabelText("Line numbers")).toBeChecked();
    expect(within(display).getByLabelText("Word wrap")).not.toBeChecked();
    expect(within(display).getByLabelText("Show whitespace")).not.toBeChecked();
    expect(within(indentation).getByLabelText("Tab size")).toBeInTheDocument();
    expect(screen.getByLabelText("Font size")).toHaveClass("select");
    expect(screen.getByLabelText("Tab size")).toHaveClass("select");
    expect(screen.getByLabelText("Font size")).toHaveAccessibleDescription(
      "Controls the size of text in the code editor. Default: 13px",
    );
    expect(
      screen.getByLabelText("Show whitespace"),
    ).toHaveAccessibleDescription(
      "Reveals spaces and tabs when indentation is unclear. Default: Off",
    );
    const reset = screen.getByRole("button", {
      name: "Reset Editor Appearance",
    });
    expect(reset).toHaveClass("button", "button--compact");
    expect(reset).toBeDisabled();
    fireEvent.change(screen.getByLabelText("Font size"), {
      target: { value: "16" },
    });
    fireEvent.change(screen.getByLabelText("Tab size"), {
      target: { value: "4" },
    });
    fireEvent.click(screen.getByLabelText("Word wrap"));
    fireEvent.click(screen.getByLabelText("Line numbers"));
    fireEvent.click(screen.getByLabelText("Show whitespace"));
    expect(useEditorSettingsStore.getState().settings).toEqual({
      "editor.fontSize": 16,
      "editor.tabSize": 4,
      "editor.wordWrap": true,
      "editor.lineNumbers": false,
      "editor.renderWhitespace": true,
    });
    expect(reset).toBeEnabled();
    fireEvent.click(reset);
    expect(useEditorSettingsStore.getState().settings).toEqual({
      "editor.fontSize": 13,
      "editor.tabSize": 2,
      "editor.wordWrap": false,
      "editor.lineNumbers": true,
      "editor.renderWhitespace": false,
    });
    expect(reset).toBeDisabled();

    await waitFor(() =>
      expect(useEditorSettingsStore.getState().persistencePhase).toBe("idle"),
    );
  });

  it("keeps settings read-only until the durable record has loaded", () => {
    useEditorSettingsStore.setState({
      persistenceReady: false,
      persistencePhase: "loading",
    });
    render(<SettingsSurface />);

    expect(screen.getByRole("region", { name: "Settings" })).toHaveAttribute(
      "aria-busy",
      "true",
    );
    expect(screen.getByRole("status")).toHaveTextContent(
      "Loading editor settings…",
    );
    expect(screen.getByLabelText("Font size")).toBeDisabled();
    expect(screen.getByLabelText("Line numbers")).toBeDisabled();
    expect(screen.getByLabelText("Word wrap")).toBeDisabled();
    expect(screen.getByLabelText("Show whitespace")).toBeDisabled();
    expect(screen.getByLabelText("Tab size")).toBeDisabled();
    expect(
      screen.getByRole("button", { name: "Reset Editor Appearance" }),
    ).toBeDisabled();
  });

  it("filters individual rows while preserving the matching group", () => {
    render(<SettingsSurface />);

    fireEvent.change(screen.getByRole("searchbox"), {
      target: { value: "whitespace" },
    });
    expect(screen.getByRole("group", { name: "Text" })).toBeInTheDocument();
    expect(screen.getByLabelText("Show whitespace")).toBeInTheDocument();
    expect(screen.queryByLabelText("Font size")).toBeNull();
    expect(screen.queryByRole("group", { name: "Editing" })).toBeNull();

    fireEvent.change(screen.getByRole("searchbox"), {
      target: { value: "numbered" },
    });
    expect(screen.getByLabelText("Line numbers")).toBeInTheDocument();
    expect(screen.queryByLabelText("Show whitespace")).toBeNull();

    fireEvent.change(screen.getByRole("searchbox"), {
      target: { value: "unclear" },
    });
    expect(screen.getByLabelText("Show whitespace")).toBeInTheDocument();
    expect(screen.queryByLabelText("Line numbers")).toBeNull();

    fireEvent.change(screen.getByRole("searchbox"), {
      target: { value: "tab size" },
    });
    expect(screen.getByRole("group", { name: "Editing" })).toBeInTheDocument();
    expect(screen.getByLabelText("Tab size")).toBeInTheDocument();
    expect(screen.queryByRole("group", { name: "Text" })).toBeNull();

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

  it("shows persistence failure without private details and exposes Retry", () => {
    const retryPersistence = vi.fn();
    useEditorSettingsStore.setState({
      persistencePhase: "error",
      persistenceErrorKind: "save",
      persistenceMessage: "Editor settings could not be saved.",
      retryPersistence,
    });

    render(<SettingsSurface />);

    expect(screen.getByRole("alert")).toHaveTextContent(
      "Editor settings could not be saved.",
    );
    fireEvent.click(screen.getByRole("button", { name: "Retry" }));
    expect(retryPersistence).toHaveBeenCalledOnce();
  });

  it("freezes Retry while the workbench is closing", () => {
    useEditorSettingsStore.setState({
      persistenceClosing: true,
      persistencePhase: "error",
      persistenceErrorKind: "save",
      persistenceMessage: "Editor settings could not be saved.",
    });

    render(<SettingsSurface />);

    expect(screen.getByRole("button", { name: "Retry" })).toBeDisabled();
  });
});
