import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { createRef } from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { AgentChatSplitter } from "./AgentChatSplitter";
import { SIDE_CHAT_SPLIT, useSideChatState } from "./sideChatState";

afterEach(() => {
  cleanup();
  useSideChatState.setState({ split: SIDE_CHAT_SPLIT });
});

function renderSplitter() {
  const panesRef = createRef<HTMLDivElement>();
  render(
    <div ref={panesRef}>
      <AgentChatSplitter panesRef={panesRef} />
    </div>,
  );
  return {
    panesRef,
    splitter: screen.getByRole("separator", { name: "Resize chat panes" }),
  };
}

const split = () => useSideChatState.getState().split;

describe("AgentChatSplitter", () => {
  it("steps by 5, or 10 with Shift, and clamps to 30-70", () => {
    useSideChatState.setState({ split: 50 });
    const { splitter } = renderSplitter();
    expect(splitter).toHaveAttribute("aria-valuenow", "50");
    expect(splitter).toHaveAttribute("aria-valuemin", "30");
    expect(splitter).toHaveAttribute("aria-valuemax", "70");

    fireEvent.keyDown(splitter, { key: "ArrowLeft" });
    expect(split()).toBe(45);
    fireEvent.keyDown(splitter, { key: "ArrowRight", shiftKey: true });
    expect(split()).toBe(55);
    fireEvent.keyDown(splitter, { key: "ArrowRight", shiftKey: true });
    fireEvent.keyDown(splitter, { key: "ArrowRight", shiftKey: true });
    expect(split()).toBe(70);
    expect(splitter).toHaveAttribute("aria-valuenow", "70");
    fireEvent.keyDown(splitter, { key: "Home" });
    expect(split()).toBe(30);
    fireEvent.keyDown(splitter, { key: "ArrowLeft" });
    expect(split()).toBe(30);
    fireEvent.keyDown(splitter, { key: "End" });
    expect(split()).toBe(70);
  });

  it("sets the split from the pointer's share of the panes", () => {
    const { panesRef, splitter } = renderSplitter();
    vi.spyOn(panesRef.current!, "getBoundingClientRect").mockReturnValue({
      left: 100,
      width: 1000,
    } as DOMRect);

    fireEvent.pointerDown(splitter, { clientX: 700 });
    expect(split()).toBe(60);
    fireEvent.pointerMove(splitter, { clientX: 1050 });
    expect(split()).toBe(70);
  });
});
