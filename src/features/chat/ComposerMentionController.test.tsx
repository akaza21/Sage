import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
} from "@testing-library/react";
import { useRef, useState } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { useWorkbenchStore } from "../../app/store";
import { setQuickOpenInvoke } from "../quickOpen/quickOpenGateway";
import { ComposerMentionController } from "./ComposerMentionController";

const initial = useWorkbenchStore.getState();
const invoke = vi.fn();

function match(relativePath: string) {
  return { rootId: "root-a", relativePath, positions: [] };
}

function answer(paths: string[]) {
  return {
    state: "ready",
    results: paths.map(match),
    indexedFiles: paths.length,
    truncated: false,
    refreshing: false,
  };
}

function Composer({ start = "" }: { start?: string }) {
  const [text, setText] = useState(start);
  const fieldRef = useRef<HTMLTextAreaElement>(null);
  return (
    <form onSubmit={(event) => event.preventDefault()}>
      <div>
        <textarea
          ref={fieldRef}
          aria-label="Message agent"
          value={text}
          onChange={(event) => setText(event.target.value)}
        />
      </div>
      <ComposerMentionController
        fieldRef={fieldRef}
        disabled={false}
        fieldId="message"
        text={text}
        onTextChange={setText}
      />
    </form>
  );
}

async function type(field: HTMLTextAreaElement, value: string) {
  await act(async () => {
    fireEvent.change(field, { target: { value } });
    field.setSelectionRange(value.length, value.length);
    fireEvent.keyUp(field);
  });
}

beforeEach(() => {
  useWorkbenchStore.setState({
    project: { id: "project-a", name: "Project" } as never,
    nativeTransitionEpoch: 4,
  });
  setQuickOpenInvoke(invoke);
});
afterEach(() => {
  cleanup();
  invoke.mockReset();
  setQuickOpenInvoke(null);
  useWorkbenchStore.setState(initial, true);
});

describe("composer @ file picker", () => {
  it("lists project files from the path index and writes the picked token", async () => {
    invoke.mockResolvedValue(answer(["src/main.rs", "docs/my notes.md"]));
    render(<Composer />);
    const field = screen.getByRole<HTMLTextAreaElement>("textbox");
    field.focus();
    await type(field, "Read @ma");
    expect(invoke).toHaveBeenLastCalledWith(
      "quick_open_query",
      expect.objectContaining({
        request: expect.objectContaining({
          projectId: "project-a",
          expectedTransitionEpoch: 4,
          query: "ma",
          limit: 20,
        }),
      }),
    );
    const options = await screen.findAllByRole("option");
    expect(options.map((option) => option.getAttribute("aria-label"))).toEqual([
      "src/main.rs",
      "docs/my notes.md",
    ]);
    expect(field).toHaveAttribute("aria-controls", "message-mentions");
    expect(field).toHaveAttribute("aria-activedescendant", options[0]!.id);
    // A plain textbox carries no aria-expanded.
    expect(field).not.toHaveAttribute("aria-expanded");
    await act(async () => {
      fireEvent.keyDown(field, { key: "ArrowDown" });
    });
    await act(async () => {
      fireEvent.keyDown(field, { key: "Enter" });
    });
    expect(field.value).toBe('Read @"docs/my notes.md" ');
    expect(screen.queryByRole("listbox")).toBeNull();
    expect(field).not.toHaveAttribute("aria-controls");
  });

  it("holds Enter while the first list loads and closes on Escape", async () => {
    let resolve: (value: unknown) => void = () => {};
    invoke.mockReturnValue(new Promise((next) => (resolve = next)));
    render(<Composer />);
    const field = screen.getByRole<HTMLTextAreaElement>("textbox");
    field.focus();
    await type(field, "@sr");
    const enter = new KeyboardEvent("keydown", {
      key: "Enter",
      bubbles: true,
      cancelable: true,
    });
    field.dispatchEvent(enter);
    expect(enter.defaultPrevented).toBe(true);
    await act(async () => resolve(answer(["src/lib.rs"])));
    expect(await screen.findByRole("option")).toHaveAccessibleName(
      "src/lib.rs",
    );
    await act(async () => {
      fireEvent.keyDown(field, { key: "Escape" });
    });
    expect(screen.queryByRole("listbox")).toBeNull();
    expect(field.value).toBe("@sr");
  });

  it("does not open for an @ inside a word", async () => {
    render(<Composer />);
    const field = screen.getByRole<HTMLTextAreaElement>("textbox");
    field.focus();
    await type(field, "mail bob@exa");
    expect(invoke).not.toHaveBeenCalled();
    expect(screen.queryByRole("listbox")).toBeNull();
  });
});
