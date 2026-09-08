import { render, screen, waitFor } from "@testing-library/react";
import { useState } from "react";
import { expect, it, vi } from "vitest";

import { LazyModuleBoundary } from "./LazyModuleBoundary";

it("retries a rejected module by invoking a fresh lazy loader", async () => {
  const load = vi
    .fn()
    .mockRejectedValueOnce(new Error("network"))
    .mockResolvedValueOnce({ default: () => <p>Loaded surface</p> });
  const error = vi.spyOn(console, "error").mockImplementation(() => undefined);
  try {
    render(
      <LazyModuleBoundary
        label="Test surface"
        load={load}
        render={(Surface) => <Surface />}
      />,
    );
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "Test surface could not be loaded.",
    );
    screen.getByRole("button", { name: "Retry" }).click();
    await waitFor(() =>
      expect(screen.getByText("Loaded surface")).toBeVisible(),
    );
    expect(load).toHaveBeenCalledTimes(2);
  } finally {
    error.mockRestore();
  }
});

it("keeps a failed module visible through an unrelated parent rerender until Retry", async () => {
  const load = vi
    .fn()
    .mockRejectedValueOnce(new Error("network"))
    .mockResolvedValueOnce({ default: () => <p>Recovered surface</p> });
  const error = vi.spyOn(console, "error").mockImplementation(() => undefined);
  function Parent() {
    const [revision, setRevision] = useState(0);
    return (
      <>
        <button type="button" onClick={() => setRevision((value) => value + 1)}>
          Unrelated rerender {revision}
        </button>
        <LazyModuleBoundary
          label="Stable failure surface"
          load={load}
          render={(Surface) => <Surface />}
        />
      </>
    );
  }
  try {
    render(<Parent />);
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "Stable failure surface could not be loaded.",
    );
    screen.getByRole("button", { name: /Unrelated rerender/ }).click();
    expect(screen.getByRole("alert")).toHaveTextContent(
      "Stable failure surface could not be loaded.",
    );
    expect(load).toHaveBeenCalledTimes(1);
    screen.getByRole("button", { name: "Retry" }).click();
    await waitFor(() =>
      expect(screen.getByText("Recovered surface")).toBeVisible(),
    );
    expect(load).toHaveBeenCalledTimes(2);
  } finally {
    error.mockRestore();
  }
});
