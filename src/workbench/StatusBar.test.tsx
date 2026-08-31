import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
} from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";

import { useEditorAppearanceStore } from "../app/editorAppearanceStore";
import { useEditorCaretStore } from "../app/editorCaretStore";
import { useWorkbenchStore } from "../app/store";
import { StatusBar } from "./StatusBar";

const initialState = useWorkbenchStore.getState();
const initialAppearance = useEditorAppearanceStore.getState();
const initialCaret = useEditorCaretStore.getState();

afterEach(() => {
  cleanup();
  useWorkbenchStore.setState(initialState, true);
  useEditorAppearanceStore.setState(initialAppearance, true);
  useEditorCaretStore.setState(initialCaret, true);
});

describe("StatusBar repository truth", () => {
  it("shows one repository branch without claiming unsupported status", () => {
    useWorkbenchStore.setState({
      repositories: {
        roots: [
          {
            rootId: "root-a",
            state: "repository",
            branch: "main",
            changes: [],
            truncated: false,
            elapsedMs: 1,
            message: null,
          },
        ],
        truncated: false,
        elapsedMs: 1,
      },
      recoveryNotice: null,
    });

    render(<StatusBar />);

    expect(screen.getByText("main")).toBeInTheDocument();
    expect(screen.queryByText("0 problems")).toBeNull();
    expect(screen.queryByText("1 session")).toBeNull();
    expect(screen.queryByText("Trusted project")).toBeNull();
    expect(
      screen.getByRole("group", { name: "Editor appearance" }),
    ).toBeInTheDocument();
    expect(screen.getByLabelText("Cycle editor font size")).toHaveTextContent(
      "Font: 13px",
    );
    expect(screen.getByLabelText("Cycle editor tab size")).toHaveTextContent(
      "Tabs: 2 spaces",
    );
    expect(screen.getByLabelText("Toggle editor word wrap")).toHaveTextContent(
      "Word wrap: Off",
    );
  });

  it("summarizes repositories across multiple roots without choosing one branch", () => {
    useWorkbenchStore.setState({
      repositories: {
        roots: [
          {
            rootId: "root-a",
            state: "repository",
            branch: "main",
            changes: [],
            truncated: false,
            elapsedMs: 1,
            message: null,
          },
          {
            rootId: "root-b",
            state: "notRepository",
            branch: null,
            changes: [],
            truncated: false,
            elapsedMs: 1,
            message: null,
          },
        ],
        truncated: false,
        elapsedMs: 2,
      },
    });

    render(<StatusBar />);

    expect(screen.getByText("1 repository across 2 roots")).toBeInTheDocument();
    expect(screen.queryByText("main")).toBeNull();
  });

  it("announces a recovery notice only when one exists", () => {
    useWorkbenchStore.setState({
      repositories: { roots: [], truncated: false, elapsedMs: 0 },
      recoveryNotice: "Recovered the last Project layout.",
    });

    render(<StatusBar />);

    expect(screen.getByRole("status")).toHaveTextContent(
      "Recovered the last Project layout.",
    );
  });

  it("cycles admitted editor font, tab, and wrap from the status bar", () => {
    render(<StatusBar />);
    fireEvent.click(screen.getByLabelText("Cycle editor font size"));
    expect(screen.getByLabelText("Cycle editor font size")).toHaveTextContent(
      "Font: 14px",
    );
    fireEvent.click(screen.getByLabelText("Cycle editor tab size"));
    expect(screen.getByLabelText("Cycle editor tab size")).toHaveTextContent(
      "Tabs: 4 spaces",
    );
    fireEvent.click(screen.getByLabelText("Toggle editor word wrap"));
    expect(screen.getByLabelText("Toggle editor word wrap")).toHaveTextContent(
      "Word wrap: On",
    );
  });

  it("keeps Editor-only appearance controls out of Agent mode", () => {
    render(<StatusBar showEditorSettings={false} />);

    expect(
      screen.queryByRole("group", { name: "Editor appearance" }),
    ).toBeNull();
    expect(screen.queryByTitle("Caret position")).toBeNull();
    expect(screen.queryByTitle("Editor language")).toBeNull();
  });

  it("names the Editor tool rail as the left sidebar", () => {
    render(<StatusBar railLeading />);
    expect(screen.getByLabelText("Hide left sidebar")).toBeInTheDocument();
  });

  it("shows an admitted File language and hides Diff or plain paths", () => {
    useWorkbenchStore.setState({
      tabs: [
        {
          id: "file:app",
          kind: "file",
          title: "app.ts",
          position: 0,
          pinned: false,
          dirty: false,
          resource: { rootId: "root-a", relativePath: "src/app.ts" },
        },
        {
          id: "diff:other",
          kind: "diff",
          title: "other.ts",
          position: 1,
          pinned: false,
          dirty: false,
          resource: { rootId: "root-a", relativePath: "src/other.ts" },
        },
        {
          id: "file:license",
          kind: "file",
          title: "LICENSE",
          position: 2,
          pinned: false,
          dirty: false,
          resource: { rootId: "root-a", relativePath: "LICENSE" },
        },
      ],
      activeTabId: "file:app",
    });
    const { rerender } = render(<StatusBar />);
    expect(screen.getByTitle("Editor language")).toHaveTextContent(
      "TypeScript",
    );

    useWorkbenchStore.setState({ activeTabId: "diff:other" });
    rerender(<StatusBar />);
    expect(screen.queryByTitle("Editor language")).toBeNull();

    useWorkbenchStore.setState({ activeTabId: "file:license" });
    rerender(<StatusBar />);
    expect(screen.queryByTitle("Editor language")).toBeNull();
  });

  it("shows an admitted caret and hides a missing one", () => {
    render(<StatusBar />);
    expect(screen.queryByTitle("Caret position")).toBeNull();
    expect(screen.queryByText("Ln 12, Col 4")).toBeNull();

    act(() => {
      useEditorCaretStore.getState().publish("one\ntwo\nthree", 9);
    });
    expect(screen.getByTitle("Caret position")).toHaveTextContent(
      "Ln 3, Col 2",
    );

    act(() => {
      useEditorCaretStore.getState().publish("one\ntwo\nthree", -1);
    });
    expect(screen.queryByTitle("Caret position")).toBeNull();
  });
});
