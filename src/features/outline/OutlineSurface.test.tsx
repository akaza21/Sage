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
import type { LanguageOutlineReport } from "../../domain/languageService";
import { languageDesktop } from "../../services/languageDesktop";
import { OutlineSurface } from "./OutlineSurface";

const resource = { rootId: "root-a", relativePath: "src/example.ts" };
const initialState = useWorkbenchStore.getState();

function report(generation: number, count: number): LanguageOutlineReport {
  return {
    resource,
    generation,
    symbols: Array.from({ length: count }, (_, index) => ({
      resource,
      name: `symbol ${index + 1}`,
      kind: "function" as const,
      startLine: index + 1,
    })),
  };
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
  cleanup();
  useWorkbenchStore.setState(initialState, true);
  vi.restoreAllMocks();
});

describe("OutlineSurface", () => {
  it("outline_surface_bounds_symbols_at_64_and_uses_the_same_language_session", async () => {
    let deliver!: (value: LanguageOutlineReport) => void;
    activateResource();
    const start = vi.spyOn(languageDesktop, "startLanguageSession");
    vi.spyOn(languageDesktop, "subscribeOutline").mockImplementation(
      async (onReport) => {
        deliver = onReport;
        return () => {};
      },
    );
    render(<OutlineSurface />);

    await waitFor(() => expect(deliver).toBeTypeOf("function"));
    act(() => deliver(report(1, 65)));

    expect(screen.getAllByRole("button")).toHaveLength(64);
    expect(start).not.toHaveBeenCalled();
    expect(
      screen.getByRole("heading", { name: "Outline" }),
    ).toBeInTheDocument();
    expect(screen.getByText("64 Symbols")).toBeInTheDocument();
  });

  it("opens the exact symbol caret through an accessible compact row", async () => {
    let deliver!: (value: LanguageOutlineReport) => void;
    activateResource();
    const openFileLocation = vi.fn(async () => undefined);
    useWorkbenchStore.setState({ openFileLocation });
    vi.spyOn(languageDesktop, "subscribeOutline").mockImplementation(
      async (onReport) => {
        deliver = onReport;
        return () => {};
      },
    );
    render(<OutlineSurface />);

    await waitFor(() => expect(deliver).toBeTypeOf("function"));
    act(() => deliver(report(1, 1)));
    const row = screen.getByRole("button", {
      name: "Function: symbol 1. src/example.ts, line 1, column 1",
    });
    expect(row).toHaveAttribute("title", "symbol 1 — src/example.ts:1:1");
    expect(row).toHaveTextContent("example.ts:1:1");
    fireEvent.click(row);

    expect(openFileLocation).toHaveBeenCalledWith({
      projectId: "project-a",
      transitionEpoch: 7,
      resource,
      lineNumber: 1,
      columnNumber: 1,
      selection: { kind: "caret" },
    });
    expect(
      screen.getByRole("heading", { name: "Outline" }),
    ).toBeInTheDocument();
    expect(screen.getByText("1 Symbol")).toBeInTheDocument();
    expect(screen.getByText("Function")).toBeInTheDocument();
  });
});
