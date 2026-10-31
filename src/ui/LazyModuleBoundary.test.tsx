import { act, render, screen, waitFor } from "@testing-library/react";
import { lazy, StrictMode, useState, type ComponentType } from "react";
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

it("shares an in-flight module across visible boundaries and renders cached modules immediately", async () => {
  let resolve!: (value: { default: ComponentType<object> }) => void;
  const load = vi.fn(
    () =>
      new Promise<{ default: ComponentType<object> }>((done) => {
        resolve = done;
      }),
  );
  const renderSurface = (Surface: ComponentType<object>) => <Surface />;
  const view = render(
    <StrictMode>
      <LazyModuleBoundary label="First" load={load} render={renderSurface} />
      <LazyModuleBoundary label="Second" load={load} render={renderSurface} />
    </StrictMode>,
  );
  await waitFor(() => expect(load).toHaveBeenCalled());
  expect(load).toHaveBeenCalledTimes(1);
  await act(async () => resolve({ default: () => <p>Shared module</p> }));
  expect(screen.getAllByText("Shared module")).toHaveLength(2);
  view.unmount();
  render(
    <LazyModuleBoundary label="Cached" load={load} render={renderSurface} />,
  );
  expect(screen.getByText("Shared module")).toBeVisible();
  expect(screen.queryByRole("status")).toBeNull();
  expect(load).toHaveBeenCalledTimes(1);
});

it("does not let an old module completion replace a newer loader", async () => {
  let finishOld!: (value: { default: ComponentType<object> }) => void;
  const oldLoad = vi.fn(
    () =>
      new Promise<{ default: ComponentType<object> }>((resolve) => {
        finishOld = resolve;
      }),
  );
  const newLoad = vi.fn(async () => ({ default: () => <p>New module</p> }));
  const renderSurface = (Surface: ComponentType<object>) => <Surface />;
  const view = render(
    <LazyModuleBoundary
      label="Switching"
      load={oldLoad}
      render={renderSurface}
    />,
  );
  await waitFor(() => expect(oldLoad).toHaveBeenCalledOnce());
  view.rerender(
    <LazyModuleBoundary
      label="Switching"
      load={newLoad}
      render={renderSurface}
    />,
  );
  expect(await screen.findByText("New module")).toBeVisible();
  await act(async () => finishOld({ default: () => <p>Old module</p> }));
  expect(screen.queryByText("Old module")).toBeNull();
  expect(screen.getByText("New module")).toBeVisible();
});

it("caches a completed module after unmount without updating its disposed view", async () => {
  let finish!: (value: { default: ComponentType<object> }) => void;
  const load = vi.fn(
    () =>
      new Promise<{ default: ComponentType<object> }>((resolve) => {
        finish = resolve;
      }),
  );
  const renderSurface = (Surface: ComponentType<object>) => <Surface />;
  const view = render(
    <LazyModuleBoundary label="Disposed" load={load} render={renderSurface} />,
  );
  await waitFor(() => expect(load).toHaveBeenCalledOnce());
  view.unmount();
  await act(async () =>
    finish({ default: () => <p>Cached after disposal</p> }),
  );
  render(
    <LazyModuleBoundary label="Remounted" load={load} render={renderSurface} />,
  );
  expect(screen.getByText("Cached after disposal")).toBeVisible();
  expect(load).toHaveBeenCalledOnce();
});

it("contains synchronous loader exceptions and retries them", async () => {
  const load = vi
    .fn<() => Promise<{ default: ComponentType<object> }>>()
    .mockImplementationOnce(() => {
      throw new Error("synchronous load failure");
    })
    .mockResolvedValueOnce({
      default: () => <p>Recovered synchronous loader</p>,
    });
  const error = vi.spyOn(console, "error").mockImplementation(() => undefined);
  try {
    render(
      <LazyModuleBoundary
        label="Synchronous"
        load={load}
        render={(Surface) => <Surface />}
      />,
    );
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "Synchronous could not be loaded.",
    );
    screen.getByRole("button", { name: "Retry" }).click();
    expect(
      await screen.findByText("Recovered synchronous loader"),
    ).toBeVisible();
    expect(load).toHaveBeenCalledTimes(2);
  } finally {
    error.mockRestore();
  }
});

it("keeps the configured fallback for a loaded surface's suspended child", async () => {
  let finish!: (value: { default: ComponentType<object> }) => void;
  const Child = lazy(
    () =>
      new Promise<{ default: ComponentType<object> }>((resolve) => {
        finish = resolve;
      }),
  );
  const load = vi.fn(async () => ({ default: () => <Child /> }));
  render(
    <LazyModuleBoundary
      label="Nested"
      load={load}
      fallback={<p role="status">Waiting for child</p>}
      render={(Surface) => <Surface />}
    />,
  );
  await waitFor(() => expect(finish).toBeTypeOf("function"));
  expect(screen.getByRole("status")).toHaveTextContent("Waiting for child");
  await act(async () => finish({ default: () => <p>Nested child ready</p> }));
  expect(screen.getByText("Nested child ready")).toBeVisible();
});
