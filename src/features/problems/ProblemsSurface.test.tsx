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
import type { LanguageDiagnosticReport } from "../../domain/languageService";
import { getWorkbenchPorts } from "../../ports/workbenchRuntime";
import { languageDesktop } from "../../services/languageDesktop";
import { ProblemsSurface } from "./ProblemsSurface";

const resource = { rootId: "root-a", relativePath: "src/example.ts" };
const initialState = useWorkbenchStore.getState();

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((next) => {
    resolve = next;
  });
  return { promise, resolve };
}

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

function activateResource(activeResource = resource) {
  useWorkbenchStore.setState({
    tabs: [
      {
        id: "file:example",
        kind: "file",
        title: activeResource.relativePath,
        position: 1,
        pinned: false,
        dirty: false,
        resource: activeResource,
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
  it("exposes listener readiness only for the current resolved subscription", async () => {
    const secondResource = {
      rootId: "root-a",
      relativePath: "src/second.ts",
    };
    const thirdResource = {
      rootId: "root-a",
      relativePath: "src/third.ts",
    };
    const first = deferred<() => void>();
    const second = deferred<() => void>();
    const third = deferred<() => void>();
    const firstUnsubscribe = vi.fn();
    const secondUnsubscribe = vi.fn();
    const thirdUnsubscribe = vi.fn();
    vi.spyOn(languageDesktop, "subscribeDiagnostics")
      .mockReturnValueOnce(first.promise)
      .mockReturnValueOnce(second.promise)
      .mockReturnValueOnce(third.promise);
    activateResource();
    const rendered = render(<ProblemsSurface />);
    const surface = screen.getByLabelText("Problems");

    expect(surface).toHaveAttribute("data-diagnostic-listener-ready", "false");
    expect(surface).toHaveAttribute("aria-busy", "true");
    expect(screen.getByText("Loading diagnostics…")).toBeVisible();
    expect(screen.queryByText("No problems for the active File.")).toBeNull();
    act(() => activateResource(secondResource));
    await waitFor(() =>
      expect(languageDesktop.subscribeDiagnostics).toHaveBeenCalledTimes(2),
    );
    await act(async () => first.resolve(firstUnsubscribe));
    expect(firstUnsubscribe).toHaveBeenCalledOnce();
    expect(surface).toHaveAttribute("data-diagnostic-listener-ready", "false");

    await act(async () => second.resolve(secondUnsubscribe));
    await waitFor(() =>
      expect(surface).toHaveAttribute("data-diagnostic-listener-ready", "true"),
    );
    expect(surface).toHaveAttribute("aria-busy", "false");
    expect(screen.getByText("No problems for the active File.")).toBeVisible();

    act(() => activateResource(thirdResource));
    await waitFor(() =>
      expect(languageDesktop.subscribeDiagnostics).toHaveBeenCalledTimes(3),
    );
    expect(surface).toHaveAttribute("data-diagnostic-listener-ready", "false");
    rendered.unmount();
    await act(async () => third.resolve(thirdUnsubscribe));
    expect(thirdUnsubscribe).toHaveBeenCalledOnce();
    expect(secondUnsubscribe).toHaveBeenCalledOnce();
  });

  it("problems_surface_bounds_diagnostics_at_64_and_replaces_by_generation", async () => {
    let deliver!: (value: LanguageDiagnosticReport) => void;
    activateResource();
    vi.spyOn(languageDesktop, "subscribeDiagnostics").mockImplementation(
      async (onReport) => {
        deliver = onReport;
        return () => {};
      },
    );
    render(<ProblemsSurface />);

    await waitFor(() => expect(deliver).toBeTypeOf("function"));
    act(() => deliver(report(1, 65)));
    expect(screen.getAllByRole("button")).toHaveLength(64);

    act(() => deliver(report(2, 1)));
    expect(screen.getAllByRole("button")).toHaveLength(1);
    expect(screen.getByRole("button")).toHaveTextContent("problem 1");
    expect(
      screen.getByRole("heading", { name: "Problems" }),
    ).toBeInTheDocument();
    expect(screen.getByText("1 Bug")).toBeInTheDocument();
    expect(screen.getByText("Bug")).toBeInTheDocument();
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
        return () => {};
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
