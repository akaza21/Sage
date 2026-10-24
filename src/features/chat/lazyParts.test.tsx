import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";

vi.mock("./TranscriptInteractions", () => ({
  TranscriptInteractions: () => <p>first part</p>,
  ComposerAnnotations: () => <p>1 annotation</p>,
}));

import { Component, type ReactNode } from "react";
import { lazyParts, TranscriptPart } from "./lazyParts";

afterEach(cleanup);

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
