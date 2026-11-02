import { act, cleanup, render, screen } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";

vi.mock("./TranscriptInteractions", () => ({
  TranscriptInteractions: () => <p>first part</p>,
  ComposerAnnotations: () => <p>1 annotation</p>,
}));

import { Component, type ReactNode } from "react";
import { lazyParts, TranscriptPart } from "./lazyParts";

afterEach(cleanup);

it("preloads without rendering, then opens a part without a fallback commit", async () => {
  const load = vi.fn(async () => ({ Card: () => <p>ready card</p> }));
  const Part = lazyParts(load);
  await Part.preload();
  expect(screen.queryByText("ready card")).toBeNull();
  render(<Part part="Card" fallback={<p>loading card</p>} />);
  expect(screen.getByText("ready card")).toBeVisible();
  expect(screen.queryByText("loading card")).toBeNull();
  expect(load).toHaveBeenCalledOnce();
});

it("shares an in-flight prewarm across intent and idle requests", async () => {
  // Every import gets its own promise, and only the first one ever resolves:
  // a consumer or prewarm that started a second import would never settle.
  const imports: Array<(chunk: { Card: () => ReactNode }) => void> = [];
  const Part = lazyParts(
    () =>
      new Promise<{ Card: () => ReactNode }>((resolve) => {
        imports.push(resolve);
      }),
  );
  const intent = Part.preload();
  const idle = Part.preload();
  render(
    <>
      <Part part="Card" fallback={<p>loading card</p>} />
      <Part part="Card" fallback={<p>loading card</p>} />
    </>,
  );
  expect(screen.getAllByText("loading card")).toHaveLength(2);

  await act(async () => {
    imports[0]({ Card: () => <p>shared card</p> });
    await Promise.all([intent, idle]);
  });

  expect(screen.getAllByText("shared card")).toHaveLength(2);
  expect(screen.queryByText("loading card")).toBeNull();
  render(<Part part="Card" fallback={<p>loading card</p>} />);
  expect(screen.getAllByText("shared card")).toHaveLength(3);
  expect(screen.queryByText("loading card")).toBeNull();
});

it("shows the fallback until the chunk first loads", () => {
  render(
    <TranscriptPart
      part="ComposerAnnotations"
      fallback={<p>loading</p>}
      contexts={[]}
      disabled={false}
      onRemove={() => undefined}
      chatId="chat"
    />,
  );
  expect(screen.getByText("loading")).toBeVisible();
});

it("retries on first use after an optional prewarm fails", async () => {
  const load = vi
    .fn<() => Promise<{ Card: () => ReactNode }>>()
    .mockRejectedValueOnce(new Error("offline"))
    .mockResolvedValue({ Card: () => <p>recovered card</p> });
  const Part = lazyParts(load);
  await expect(Part.preload()).rejects.toThrow("offline");
  render(<Part part="Card" />);
  expect(await screen.findByText("recovered card")).toBeVisible();
  expect(load).toHaveBeenCalledTimes(2);
});

it("renders a later part of an already loaded chunk in the same commit", async () => {
  const { rerender } = render(
    <TranscriptPart
      part="TranscriptInteractions"
      chatId="chat"
      focused
      contentRef={{ current: null }}
      initialKeyboard={false}
    />,
  );
  expect(await screen.findByText("first part")).toBeVisible();
  rerender(
    <>
      <TranscriptPart
        part="TranscriptInteractions"
        chatId="chat"
        focused
        contentRef={{ current: null }}
        initialKeyboard={false}
      />
      <TranscriptPart
        part="ComposerAnnotations"
        contexts={[]}
        disabled={false}
        onRemove={() => undefined}
        chatId="chat"
      />
    </>,
  );
  // No Suspense round trip: the composer chip shows with the comment field.
  expect(screen.getByText("1 annotation")).toBeVisible();
});

class Catch extends Component<{ children: ReactNode }, { failed: boolean }> {
  state = { failed: false };
  static getDerivedStateFromError() {
    return { failed: true };
  }
  render() {
    return this.state.failed ? <p>load failed</p> : this.props.children;
  }
}

it("imports again on the next mount after a chunk fails to load", async () => {
  vi.spyOn(console, "error").mockImplementation(() => undefined);
  const load = vi
    .fn<() => Promise<{ Card: () => ReactNode }>>()
    .mockRejectedValueOnce(new Error("offline"))
    .mockResolvedValue({ Card: () => <p>card</p> });
  const Part = lazyParts(load);
  const first = render(
    <Catch>
      <Part part="Card" />
    </Catch>,
  );
  expect(await screen.findByText("load failed")).toBeVisible();
  first.unmount();
  render(
    <Catch>
      <Part part="Card" />
    </Catch>,
  );
  expect(await screen.findByText("card")).toBeVisible();
  expect(load).toHaveBeenCalledTimes(2);
});
