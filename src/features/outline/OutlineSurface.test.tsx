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
        return vi.fn();
      },
    );
    render(<OutlineSurface />);

    await waitFor(() => expect(deliver).toBeTypeOf("function"));
    act(() => deliver(report(1, 65)));

    expect(screen.getAllByRole("button")).toHaveLength(64);
    expect(start).not.toHaveBeenCalled();
  });

  it("problems_or_outline_row_opens_the_existing_file_tab", async () => {
    let deliver!: (value: LanguageOutlineReport) => void;
    activateResource();
    const openFile = vi.fn(async () => undefined);
    useWorkbenchStore.setState({ openFile });
    vi.spyOn(languageDesktop, "subscribeOutline").mockImplementation(
      async (onReport) => {
        deliver = onReport;
        return vi.fn();
      },
    );
    render(<OutlineSurface />);

    await waitFor(() => expect(deliver).toBeTypeOf("function"));
    act(() => deliver(report(1, 1)));
    fireEvent.click(screen.getByRole("button", { name: /symbol 1/i }));

    expect(openFile).toHaveBeenCalledWith(resource);
  });
});
