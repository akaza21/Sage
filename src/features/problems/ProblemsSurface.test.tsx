import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { useWorkbenchStore } from "../../app/store";
import type { LanguageDiagnosticReport } from "../../domain/languageService";
import { getWorkbenchPorts } from "../../ports/workbenchRuntime";
import { languageDesktop } from "../../services/languageDesktop";
import { ProblemsSurface } from "./ProblemsSurface";

const resource = { rootId: "root-a", relativePath: "src/example.ts" };
const initialState = useWorkbenchStore.getState();

function report(generation: number, count: number): LanguageDiagnosticReport {
  return {
    resource,
    generation,
    diagnostics: Array.from({ length: count }, (_, index) => ({
      resource,
      message: `problem ${index + 1}`,
      severity: "error" as const,
      startLine: index + 1,
      startColumn: 1,
      endLine: index + 1,
      endColumn: 2,
    })),
  };
}

function activateResource() {
  useWorkbenchStore.setState({
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

describe("ProblemsSurface", () => {
  it("problems_surface_bounds_diagnostics_at_64_and_replaces_by_generation", async () => {
    let deliver!: (value: LanguageDiagnosticReport) => void;
    activateResource();
    vi.spyOn(languageDesktop, "subscribeDiagnostics").mockImplementation(
      async (onReport) => {
        deliver = onReport;
        return vi.fn();
      },
    );
    render(<ProblemsSurface />);

    await waitFor(() => expect(deliver).toBeTypeOf("function"));
    act(() => deliver(report(1, 65)));
    expect(screen.getAllByRole("button")).toHaveLength(64);

    act(() => deliver(report(2, 1)));
    expect(screen.getAllByRole("button")).toHaveLength(1);
    expect(screen.getByRole("button")).toHaveTextContent("problem 1");
  });

  it("inactive_problems_or_outline_stops_retaining_reports", async () => {
    let deliver!: (value: LanguageDiagnosticReport) => void;
    const unsubscribe = vi.fn();
    activateResource();
    vi.spyOn(languageDesktop, "subscribeDiagnostics").mockImplementation(
      async (onReport) => {
        deliver = onReport;
        return unsubscribe;
      },
    );
    const Harness = ({ active }: { active: boolean }) =>
      active ? <ProblemsSurface /> : null;
    const rendered = render(<Harness active />);

    await waitFor(() => expect(deliver).toBeTypeOf("function"));
    act(() => deliver(report(1, 1)));
    expect(screen.getByRole("button")).toBeInTheDocument();
    rendered.rerender(<Harness active={false} />);
    act(() => deliver(report(2, 65)));

    expect(unsubscribe).toHaveBeenCalledOnce();
    expect(screen.queryByLabelText("Problems")).not.toBeInTheDocument();
  });

  it("problems_or_outline_row_opens_the_existing_file_tab", async () => {
    let deliver!: (value: LanguageDiagnosticReport) => void;
    activateResource();
    const openFile = vi.fn(async () => undefined);
    useWorkbenchStore.setState({ openFile });
    const openEditorDocument = vi.spyOn(
      getWorkbenchPorts().editor,
      "openEditorDocument",
    );
    vi.spyOn(languageDesktop, "subscribeDiagnostics").mockImplementation(
      async (onReport) => {
        deliver = onReport;
        return vi.fn();
      },
    );
    render(<ProblemsSurface />);

    await waitFor(() => expect(deliver).toBeTypeOf("function"));
    act(() => deliver(report(1, 1)));
    fireEvent.click(screen.getByRole("button", { name: /problem 1/i }));

    expect(openFile).toHaveBeenCalledWith(resource);
    expect(openEditorDocument).not.toHaveBeenCalled();
  });
});
