import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { useWorkbenchStore } from "../../app/store";
import type { EditorDocument } from "../../domain/editor";
import { OutlineSurface } from "./OutlineSurface";
import { MAX_OUTLINE_SOURCE_CHARS } from "./outlineAvailability";
import { MAX_OUTLINE_SYMBOLS } from "./syntaxOutline";
import { OUTLINE_SETTLE_MS } from "./useSyntaxOutline";

const resource = { rootId: "root-a", relativePath: "src/example.ts" };
const initialState = useWorkbenchStore.getState();

function setContent(content: string, target = resource) {
  const version = {
    fingerprint: "f",
    byteLen: content.length,
    modifiedAt: null,
  };
  const document: EditorDocument = {
    resource: target,
    disk: version,
    base: version,
    buffer: { fingerprint: "f", byteLen: content.length, revision: 0, content },
    lineEnding: "lf",
    dirty: false,
    conflict: null,
  };
  useWorkbenchStore.setState({
    activeEditor: {
      phase: "ready",
      resource: target,
      document,
      content,
      revision: 0,
      persistedRevision: 0,
      persisting: false,
      saving: false,
      message: null,
    },
  });
}

function activateResource() {
  useWorkbenchStore.setState({
    project: {
      id: "project-a",
      name: "Project A",
      roots: [
        {
          id: "root-a",
          name: "Root A",
          ordinal: 0,
          availability: "available",
          message: null,
        },
      ],
    },
    projectGeneration: 3,
    nativeTransitionEpoch: 7,
    projectTransitioning: false,
    tabs: [
      {
        id: "file:example",
        kind: "file",
        title: "example.ts",
        position: 1,
        pinned: false,
        dirty: false,
        resource,
      },
    ],
    activeTabId: "file:example",
  });
}

afterEach(() => {
  // Restores real timers even when a fake-timer test fails mid-way.
  vi.useRealTimers();
  cleanup();
  useWorkbenchStore.setState(initialState, true);
  vi.restoreAllMocks();
});

describe("OutlineSurface", () => {
  it("lists the active File's top-level declarations from its syntax tree", async () => {
    activateResource();
    setContent(
      "import x from './x';\nexport function first() {}\n\nclass Second {}\nconst third = 1;\n",
    );
    render(<OutlineSurface />);

    expect(await screen.findByText("3 Symbols")).toBeInTheDocument();
    expect(
      screen.getByRole("heading", { name: "Outline" }),
    ).toBeInTheDocument();
    expect(
      screen
        .getAllByRole("button")
        .map((row) => row.getAttribute("aria-label")),
    ).toEqual([
      "Function: first. src/example.ts, line 2, column 17",
      "Class: Second. src/example.ts, line 4, column 7",
      "Variable: third. src/example.ts, line 5, column 7",
    ]);
  });

  it("opens the declaration's exact caret through an accessible compact row", async () => {
    activateResource();
    const openFileLocation = vi.fn(async () => undefined);
    useWorkbenchStore.setState({ openFileLocation });
    setContent("\nexport function example() {}\n");
    render(<OutlineSurface />);

    const row = await screen.findByRole("button", {
      name: "Function: example. src/example.ts, line 2, column 17",
    });
    expect(row).toHaveAttribute("title", "example — src/example.ts:2:17");
    expect(row).toHaveTextContent("example.ts:2:17");
    fireEvent.click(row);

    expect(openFileLocation).toHaveBeenCalledWith({
      projectId: "project-a",
      transitionEpoch: 7,
      resource,
      lineNumber: 2,
      columnNumber: 17,
      selection: { kind: "caret" },
    });
    expect(screen.getByText("1 Symbol")).toBeInTheDocument();
    expect(screen.getByText("Function")).toBeInTheDocument();
  });

  it("keeps the last Outline while typing and rebuilds once edits settle", async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
    activateResource();
    setContent("const one = 1;\n");
    render(<OutlineSurface />);
    expect(await screen.findByText("1 Symbol")).toBeInTheDocument();

    act(() => setContent("const one = 1;\nconst two = 2;\n"));
    expect(screen.getByText("1 Symbol")).toBeInTheDocument();
    await act(async () => {
      await vi.advanceTimersByTimeAsync(OUTLINE_SETTLE_MS);
    });
    await waitFor(() =>
      expect(screen.getByText("2 Symbols")).toBeInTheDocument(),
    );
  });

  it("says when the list stops at its cap", async () => {
    activateResource();
    setContent(
      Array.from(
        { length: MAX_OUTLINE_SYMBOLS + 1 },
        (_, index) => `const value${index} = ${index};`,
      ).join("\n"),
    );
    render(<OutlineSurface />);

    expect(
      await screen.findByText(`First ${MAX_OUTLINE_SYMBOLS} Symbols`),
    ).toBeInTheDocument();
    expect(screen.getAllByRole("button")).toHaveLength(MAX_OUTLINE_SYMBOLS);
  });

  it("shows an honest empty state for a File type without an Outline", async () => {
    activateResource();
    setContent("plain notes", { rootId: "root-a", relativePath: "notes.txt" });
    render(<OutlineSurface />);

    expect(
      await screen.findByText("No Outline for this File type."),
    ).toBeInTheDocument();
    expect(screen.queryAllByRole("button")).toHaveLength(0);
  });

  it("says so when a File is over the Outline limit", async () => {
    activateResource();
    setContent("x".repeat(MAX_OUTLINE_SOURCE_CHARS + 1));
    render(<OutlineSurface />);
    expect(
      await screen.findByText("This File is too large for an Outline."),
    ).toBeInTheDocument();
  });

  it("says it is reading while a new File's first Outline is built", async () => {
    activateResource();
    setContent("const one = 1;\n");
    render(<OutlineSurface />);
    expect(screen.getByText("Reading symbols…")).toBeInTheDocument();
    expect(await screen.findByText("1 Symbol")).toBeInTheDocument();
  });

  it("says no symbols for a supported File that declares none", async () => {
    activateResource();
    setContent('import "./side-effect";\n');
    render(<OutlineSurface />);
    expect(
      await screen.findByText("No symbols for the active File."),
    ).toBeInTheDocument();
  });

  it("clears when the active File closes", async () => {
    activateResource();
    setContent("const one = 1;\n");
    render(<OutlineSurface />);
    expect(await screen.findByText("1 Symbol")).toBeInTheDocument();

    act(() => useWorkbenchStore.setState({ activeEditor: null }));
    expect(screen.getByText("No File is open.")).toBeInTheDocument();
  });
});
