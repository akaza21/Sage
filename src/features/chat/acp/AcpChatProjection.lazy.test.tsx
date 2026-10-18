import { act, cleanup, render, screen } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";

import { page } from "./acpProjection.fixture";

afterEach(() => {
  cleanup();
  vi.doUnmock("./AcpChatPresentation");
  vi.unstubAllGlobals();
});

it.each([1888, 720, 480])(
  "resolves cold Chat history after completion and remounts at width %i",
  async (width) => {
    vi.resetModules();
    vi.stubGlobal("innerWidth", width);
    let release!: () => void;
    const pending = new Promise<void>((resolve) => {
      release = resolve;
    });
    vi.doMock("./AcpChatPresentation", async (importOriginal) => {
      await pending;
      return importOriginal();
    });
    const { AcpChatProjection } = await import("./AcpChatProjection");
    const view = render(<AcpChatProjection page={page("running")} active />);
    expect(screen.getByText(/Loading Chat history/)).toBeVisible();
    const completed = page("completed");
    view.rerender(<AcpChatProjection page={completed} active />);
    expect(screen.queryByText("Normalized answer")).not.toBeInTheDocument();
    expect(screen.getByText(/Loading Chat history/)).toBeVisible();
    view.rerender(<AcpChatProjection page={completed} active={false} />);
    view.rerender(<AcpChatProjection page={completed} active />);
    expect(screen.getByText(/Loading Chat history/)).toBeVisible();
    await act(async () => {
      release();
      await vi.dynamicImportSettled();
    });
    expect(await screen.findByText("Normalized answer")).toBeVisible();
    expect(screen.queryByText(/Loading Chat history/)).not.toBeInTheDocument();
    view.rerender(<AcpChatProjection page={completed} active={false} />);
    view.rerender(<AcpChatProjection page={completed} active />);
    expect(screen.getByText("Normalized answer")).toBeVisible();
    expect(screen.queryByText(/Loading Chat history/)).not.toBeInTheDocument();
  },
);
