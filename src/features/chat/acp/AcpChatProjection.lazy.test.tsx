import { act, cleanup, render, screen } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";

import { settleLazyModules } from "../../../test/settleLazyModules";
import { page } from "./acpProjection.fixture";
// Transformed once with the file. Each test resets modules, so the chunk the
// mock holds is still loaded anew, but not compiled inside the test's time.
import "./AcpChatPresentation";

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
    // dynamicImportSettled does not see a mock factory, so the test holds the
    // boundary's load itself: requested once its import reaches the factory.
    let requested!: (request: { load: Promise<unknown> }) => void;
    const boundaryRequest = new Promise<{ load: Promise<unknown> }>(
      (resolve) => {
        requested = resolve;
      },
    );
    vi.doMock("./AcpChatPresentation", (importOriginal) => {
      const load = pending.then(() => importOriginal());
      requested({ load });
      return load;
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
    // Wait for the boundary's held load rather than race a cold load against
    // findByText's clock.
    const { load } = await boundaryRequest;
    await act(async () => {
      release();
      await load;
    });
    await settleLazyModules();
    expect(screen.getByText("Normalized answer")).toBeVisible();
    expect(screen.queryByText(/Loading Chat history/)).not.toBeInTheDocument();
    view.rerender(<AcpChatProjection page={completed} active={false} />);
    view.rerender(<AcpChatProjection page={completed} active />);
    expect(screen.getByText("Normalized answer")).toBeVisible();
    expect(screen.queryByText(/Loading Chat history/)).not.toBeInTheDocument();
  },
);
