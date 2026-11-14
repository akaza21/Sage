import { act, cleanup, render, screen } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";

vi.mock("./TranscriptInteractions", () => ({
  TranscriptInteractions: () => <p>first part</p>,
  ComposerAnnotations: () => <p>1 annotation</p>,
}));

import { Component, type ReactNode } from "react";
import { ChatHandoffControl, lazyParts, TranscriptPart } from "./lazyParts";

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
  // A later prewarm resolves from the finished import without importing
  // again; a second import would never resolve.
  await Part.preload();
  expect(imports).toHaveLength(1);
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

it("keeps the surface up when a chunk fails to load, then imports again on the next mount", async () => {
  vi.spyOn(console, "error").mockImplementation(() => undefined);
  const load = vi
    .fn<() => Promise<{ Card: () => ReactNode }>>()
    .mockRejectedValueOnce(new Error("offline"))
    .mockResolvedValue({ Card: () => <p>card</p> });
  const Part = lazyParts(load);
  const first = render(
    <Catch>
      <p>chat surface</p>
      <Part part="Card" fallback={<p>card unavailable</p>} />
    </Catch>,
  );
  await act(async () => {
    await expect(Part.preload()).rejects.toThrow("offline");
  });
  expect(await screen.findByText("card unavailable")).toBeVisible();
  expect(screen.getByText("chat surface")).toBeVisible();
  expect(screen.queryByText("load failed")).toBeNull();
  first.unmount();
  render(
    <Catch>
      <Part part="Card" />
    </Catch>,
  );
  expect(await screen.findByText("card")).toBeVisible();
  expect(load).toHaveBeenCalledTimes(2);
});

it("renders nothing for a part without a fallback whose chunk failed", async () => {
  vi.spyOn(console, "error").mockImplementation(() => undefined);
  const Part = lazyParts<{ Card: () => ReactNode }>(() =>
    Promise.reject(new Error("offline")),
  );
  render(
    <Catch>
      <p>chat surface</p>
      <Part part="Card" />
    </Catch>,
  );
  await act(async () => {
    await expect(Part.preload()).rejects.toThrow("offline");
  });
  await act(async () => undefined);
  expect(screen.getByText("chat surface")).toBeVisible();
  expect(screen.queryByText("load failed")).toBeNull();
});

it("leaves an error a loaded part throws to the boundary above", async () => {
  vi.spyOn(console, "error").mockImplementation(() => undefined);
  const Part = lazyParts(async () => ({
    Card: (): ReactNode => {
      throw new Error("render bug");
    },
  }));
  render(
    <Catch>
      <Part part="Card" fallback={<p>card unavailable</p>} />
    </Catch>,
  );
  expect(await screen.findByText("load failed")).toBeVisible();
  expect(screen.queryByText("card unavailable")).toBeNull();
});

it("holds the Hand off trigger's place until it loads, so the header never shifts", async () => {
  const { container } = render(
    <ChatHandoffControl
      acpPage={null}
      messages={[]}
      busy={false}
      onCommitted={() => undefined}
    />,
  );
  const placeholder = container.querySelector<HTMLElement>(".handoff-control");
  expect(placeholder).not.toBeNull();
  expect(placeholder!.style.visibility).toBe("hidden");
  expect(placeholder!.querySelector("button")).toHaveTextContent("Hand off");
  expect(screen.queryByRole("button", { name: "Hand off chat" })).toBeNull();
  expect(
    await screen.findByRole("button", { name: "Hand off chat" }),
  ).toBeInTheDocument();
  expect(container.querySelectorAll(".handoff-control")).toHaveLength(1);
});
