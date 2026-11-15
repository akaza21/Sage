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
// The Outline loads the TypeScript parser lazily. Load it with the file, so
// the first test's findByText does not race its cold import.
import "../files/editorLanguageJavaScript";
const OUTLINE_SETTLE_MS = 200;

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
        .getAllByRole("treeitem")
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

    const row = await screen.findByRole("treeitem", {
      name: "Function: example. src/example.ts, line 2, column 17",
    });
    expect(row).toHaveAttribute("title", "example — src/example.ts:2:17");
    expect(row).not.toHaveTextContent(":2:17");
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

  it("nests members under their class and collapses them from the keyboard or the chevron", async () => {
    activateResource();
    setContent(
      "class Shape {\n  area() {}\n  name = 1;\n}\nconst after = 1;\n",
    );
    const { container } = render(<OutlineSurface />);

    const shape = await screen.findByRole("treeitem", {
      name: /^Class: Shape/,
    });
    const names = () =>
      screen
        .getAllByRole("treeitem")
        .map((row) => [
          row.querySelector(".outline-card__name")?.textContent,
          row.getAttribute("aria-level"),
        ]);
    expect(shape).toHaveAttribute("aria-expanded", "true");
    expect(names()).toEqual([
      ["Shape", "1"],
      ["area", "2"],
      ["name", "2"],
      ["after", "1"],
    ]);
    expect(
      screen.getByRole("treeitem", { name: /^Variable: after/ }),
    ).not.toHaveAttribute("aria-expanded");

    fireEvent.keyDown(shape, { key: "ArrowLeft" });
    expect(shape).toHaveAttribute("aria-expanded", "false");
    expect(names()).toEqual([
      ["Shape", "1"],
      ["after", "1"],
    ]);

    fireEvent.keyDown(shape, { key: "ArrowRight" });
    expect(names()).toHaveLength(4);

    const twisty = container.querySelector(".outline-row__twisty");
    expect(twisty).not.toBeNull();
    fireEvent.click(twisty!);
    expect(names()).toHaveLength(2);
  });

  it("keeps a collapsed row collapsed through edits and forgets rows the File no longer declares", async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
    activateResource();
    const body = " {\n  area() {}\n}\nclass Box {\n  side() {}\n}\n";
    setContent(`class Shape${body}`);
    render(<OutlineSurface />);
    const row = (name: string) =>
      screen.findByRole("treeitem", { name: new RegExp(`^Class: ${name}`) });
    const edit = async (content: string) => {
      act(() => setContent(content));
      await act(async () => {
        await vi.advanceTimersByTimeAsync(OUTLINE_SETTLE_MS);
      });
    };

    fireEvent.keyDown(await row("Shape"), { key: "ArrowLeft" });
    expect(await row("Shape")).toHaveAttribute("aria-expanded", "false");

    await edit(`// one\n// two\nclass Shape${body}`);
    await waitFor(async () =>
      expect(await row("Shape")).toHaveAttribute("aria-expanded", "false"),
    );

    await edit(`// one\n// two\nclass Renamed${body}`);
    await row("Renamed");
    fireEvent.keyDown(await row("Box"), { key: "ArrowLeft" });
    await edit(`// one\n// two\nclass Shape${body}`);
    await waitFor(async () =>
      expect(await row("Shape")).toHaveAttribute("aria-expanded", "true"),
    );
    expect(await row("Box")).toHaveAttribute("aria-expanded", "false");
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
    vi.useFakeTimers({ toFake: ["setTimeout", "clearTimeout"] });
    activateResource();
    setContent(
      Array.from(
        { length: MAX_OUTLINE_SYMBOLS + 1 },
        (_, index) => `const value${index} = ${index};`,
      ).join("\n"),
    );
    render(<OutlineSurface />);

    // The Outline parses in slices between timers, then renders 500 rows:
    // run it to the end rather than poll it against a one-second clock.
    await act(() => vi.runAllTimersAsync());
    expect(
      screen.getByText(`First ${MAX_OUTLINE_SYMBOLS} Symbols`),
    ).toBeInTheDocument();
    // Counting skips the visibility check, which runs jsdom's style cascade
    // for each of the 500 rows and their ancestors: seconds under load.
    const rows = screen.getAllByRole("treeitem", { hidden: true });
    expect(rows).toHaveLength(MAX_OUTLINE_SYMBOLS);
    expect(rows[0]).toBeVisible();
    expect(rows.at(-1)).toBeVisible();
  });

  it("shows an honest empty state for a File type without an Outline", async () => {
    activateResource();
    setContent("plain notes", { rootId: "root-a", relativePath: "notes.txt" });
    render(<OutlineSurface />);

    expect(
      await screen.findByText("No outline for this file type."),
    ).toBeInTheDocument();
    expect(screen.queryAllByRole("button")).toHaveLength(0);
  });

  it("says so when a File is over the Outline limit", async () => {
    activateResource();
    setContent("x".repeat(MAX_OUTLINE_SOURCE_CHARS + 1));
    render(<OutlineSurface />);
    expect(
      await screen.findByText("This file is too large for an outline."),
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
      await screen.findByText("No symbols for the active file."),
    ).toBeInTheDocument();
  });

  it("clears when the active File closes", async () => {
    activateResource();
    setContent("const one = 1;\n");
    render(<OutlineSurface />);
    expect(await screen.findByText("1 Symbol")).toBeInTheDocument();

    act(() => useWorkbenchStore.setState({ activeEditor: null }));
    expect(screen.getByText("No file is open.")).toBeInTheDocument();
  });
});
