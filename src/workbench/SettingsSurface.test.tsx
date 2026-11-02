import { useAgentSettingsState } from "../features/settings/agentSettingsState";
import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { useEditorSettingsStore } from "../app/editorSettingsStore";
import { useSettingsPageRequest } from "../app/settingsPageRequest";
import { DEFAULT_EDITOR_SETTINGS } from "../domain/editorSettingsCatalogue";
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
  useSettingsPageRequest.setState({ page: null });
  useAgentSettingsState.getState().close();
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
    ).toEqual(["Editor", "Keyboard", "Browser"]);
    expect(within(nav).getByRole("tab", { name: "Editor" })).toHaveAttribute(
      "aria-selected",
      "true",
    );
  });

  it("opens on a requested page and follows a request while open", async () => {
    useSettingsPageRequest.setState({ page: "keyboard" });
    render(<SettingsSurface />);
    expect(screen.getByRole("tab", { name: "Keyboard" })).toHaveAttribute(
      "aria-selected",
      "true",
    );
    expect(
      await screen.findByRole("heading", { name: "Keyboard Shortcuts" }),
    ).toBeInTheDocument();
    await waitFor(() =>
      expect(useSettingsPageRequest.getState().page).toBeNull(),
    );

    fireEvent.change(screen.getByRole("searchbox"), {
      target: { value: "tab size" },
    });
    act(() => useSettingsPageRequest.setState({ page: "keyboard" }));
    expect(screen.getByRole("tab", { name: "Keyboard" })).toHaveAttribute(
      "aria-selected",
      "true",
    );
    expect(screen.getByRole("searchbox")).toHaveValue("");
    expect(useSettingsPageRequest.getState().page).toBeNull();
  });

  it("opens Browser in the shared Agent settings dialog", async () => {
    render(<SettingsSurface />);
    fireEvent.click(screen.getByRole("tab", { name: "Browser" }));
    await waitFor(() =>
      expect(useAgentSettingsState.getState().page).toBe("Browser"),
    );
    expect(screen.queryByRole("heading", { name: "Browser" })).toBeNull();
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

  it("explains, changes, and resets every Editor setting", async () => {
    render(<SettingsSurface />);

    expect(
      screen.getByRole("heading", { name: "Editor Appearance" }),
    ).toBeInTheDocument();
    expect(screen.getByText("Default: 13px")).toBeInTheDocument();
    expect(screen.getByText("Default: 2 spaces")).toBeInTheDocument();
    expect(screen.getAllByText("Default: On")).toHaveLength(8);
    expect(screen.getAllByText("Default: Off")).toHaveLength(2);
    expect(screen.getByText("Default: Alt")).toBeInTheDocument();
    expect(screen.getByText("Default: Dimmed")).toBeInTheDocument();
    const files = screen.getByRole("group", { name: "Files" });
    const search = screen.getByRole("group", { name: "Search" });
    expect(within(search).getByLabelText("Use ignore files")).toBeChecked();
    const git = screen.getByRole("group", { name: "Git" });
    expect(within(git).getByLabelText("Auto refresh")).toBeChecked();
    expect(within(git).getByLabelText("Decorations")).toBeChecked();
    const ignored = within(files).getByLabelText("Ignored files in Explorer");
    expect(
      within(ignored)
        .getAllByRole("option")
        .map((option) => option.textContent),
    ).toEqual(["Dimmed", "Hidden"]);
    expect(screen.getByRole("searchbox")).toHaveClass("input");
    const display = screen.getByRole("group", { name: "Text" });
    const indentation = screen.getByRole("group", { name: "Editing" });
    expect(within(display).getByLabelText("Font size")).toBeInTheDocument();
    expect(within(display).getByLabelText("Line numbers")).toBeChecked();
    expect(within(display).getByLabelText("Word wrap")).not.toBeChecked();
    expect(within(display).getByLabelText("Show whitespace")).not.toBeChecked();
    expect(within(indentation).getByLabelText("Tab size")).toBeInTheDocument();
    expect(within(display).getByLabelText("Match brackets")).toBeChecked();
    expect(within(display).getByLabelText("Code folding")).toBeChecked();
    expect(
      within(display).getByLabelText("Highlight occurrences"),
    ).toBeChecked();
    expect(
      within(indentation).getByLabelText("Auto-close brackets"),
    ).toBeChecked();
    const modifier = within(indentation).getByLabelText(
      "Multi-cursor modifier",
    );
    expect(modifier).toHaveClass("select");
    expect(
      within(modifier)
        .getAllByRole("option")
        .map((option) => option.textContent),
    ).toEqual(["Alt", "Ctrl"]);
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
    fireEvent.click(screen.getByLabelText("Code folding"));
    fireEvent.change(modifier, { target: { value: "ctrlCmd" } });
    fireEvent.click(within(search).getByLabelText("Use ignore files"));
    fireEvent.change(ignored, { target: { value: "hidden" } });
    expect(useEditorSettingsStore.getState().settings).toEqual({
      ...DEFAULT_EDITOR_SETTINGS,
      "search.useIgnoreFiles": false,
      "explorer.showIgnored": "hidden",
      "editor.folding": false,
      "editor.multiCursorModifier": "ctrlCmd",
      "editor.fontSize": 16,
      "editor.tabSize": 4,
      "editor.wordWrap": true,
      "editor.lineNumbers": false,
      "editor.renderWhitespace": true,
    });
    expect(reset).toBeEnabled();
    fireEvent.click(reset);
    expect(useEditorSettingsStore.getState().settings).toEqual(
      DEFAULT_EDITOR_SETTINGS,
    );
    expect(reset).toBeDisabled();

    await waitFor(() =>
      expect(useEditorSettingsStore.getState().persistencePhase).toBe("idle"),
    );
  });

  it("edits the files.exclude pattern list and refuses patterns the store would refuse", async () => {
    render(<SettingsSurface />);
    const files = screen.getByRole("group", { name: "Files" });
    const list = within(files).getByRole("list", { name: "Exclude" });
    expect(
      within(list)
        .getAllByRole("listitem")
        .map((item) => item.textContent),
    ).toEqual([
      "**/.git",
      "**/.svn",
      "**/.hg",
      "**/CVS",
      "**/.DS_Store",
      "**/Thumbs.db",
    ]);
    const input = within(files).getByLabelText("Add to Exclude");
    const add = within(files).getByRole("button", { name: "Add Pattern" });

    fireEvent.change(input, { target: { value: "  **/dist  " } });
    fireEvent.click(add);
    expect(useEditorSettingsStore.getState().settings["files.exclude"]).toEqual(
      [
        "**/.git",
        "**/.svn",
        "**/.hg",
        "**/CVS",
        "**/.DS_Store",
        "**/Thumbs.db",
        "**/dist",
      ],
    );
    expect(input).toHaveValue("");

    for (const [pattern, message] of [
      ["**/dist", "That pattern is already listed."],
      ["/dist", "Write the pattern relative to the Project"],
      ["", "Enter a pattern."],
    ] as const) {
      fireEvent.change(input, { target: { value: pattern } });
      fireEvent.submit(input);
      expect(input).toHaveAccessibleDescription(
        expect.stringContaining(message),
      );
      expect(input).toHaveAttribute("aria-invalid", "true");
    }
    expect(
      useEditorSettingsStore.getState().settings["files.exclude"],
    ).toHaveLength(7);

    fireEvent.click(
      within(files).getByRole("button", { name: "Remove **/.git" }),
    );
    // Focus moves to the pattern that took the removed one's place.
    expect(
      within(files).getByRole("button", { name: "Remove **/.svn" }),
    ).toHaveFocus();
    expect(
      useEditorSettingsStore.getState().settings["files.exclude"],
    ).not.toContain("**/.git");
    const reset = screen.getByRole("button", {
      name: "Reset Editor Appearance",
    });
    expect(reset).toBeEnabled();
    fireEvent.click(reset);
    expect(useEditorSettingsStore.getState().settings["files.exclude"]).toEqual(
      DEFAULT_EDITOR_SETTINGS["files.exclude"],
    );
    expect(reset).toBeDisabled();
    await waitFor(() =>
      expect(useEditorSettingsStore.getState().persistencePhase).toBe("idle"),
    );
  });

  it("stops adding patterns at the list's cap", () => {
    useEditorSettingsStore.setState({
      settings: {
        ...useEditorSettingsStore.getState().settings,
        "files.exclude": Array.from({ length: 50 }, (_, index) => `p${index}`),
      },
    });
    render(<SettingsSurface />);
    const files = screen.getByRole("group", { name: "Files" });
    expect(within(files).getByLabelText("Add to Exclude")).toHaveAttribute(
      "readonly",
    );
    expect(
      within(files).getByRole("button", { name: "Add Pattern" }),
    ).toBeDisabled();
    expect(
      within(files).getByText("The list holds at most 50 patterns."),
    ).toBeInTheDocument();
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
