import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import type { TranscriptQuoteContext } from "../../domain/transcriptQuote";
import { AnnotationComment } from "./AnnotationComment";

const quote = {
  kind: "transcriptQuote" as const,
  ordinal: 1,
  sourceTurnId: "t",
  sourceItemId: "i",
  text: "answer",
  startOffset: 0,
  endOffset: 6,
};

afterEach(cleanup);

function mount(current: TranscriptQuoteContext = quote) {
  const onSave = vi.fn();
  const content = document.createElement("div");
  const scrollport = document.createElement("div");
  scrollport.append(content);
  const view = render(
    <AnnotationComment
      quote={current}
      content={content}
      scrollport={scrollport}
      onSave={onSave}
    />,
  );
  return {
    onSave,
    view,
    input: screen.getByRole("textbox", { name: "Annotation comment" }),
  };
}

it("autofocuses the exact optional-comment pill and saves on Enter", () => {
  const { onSave, input } = mount();
  expect(input).toHaveFocus();
  expect(input).toHaveAttribute("placeholder", "Add an optional comment…");
  expect(
    screen.getByRole("form", { name: "Annotation comment" }),
  ).toBeVisible();
  fireEvent.change(input, { target: { value: "Why?" } });
  fireEvent.submit(input.closest("form")!);
  expect(onSave).toHaveBeenCalledWith("Why?", true);
});

it("saves with the check button", () => {
  const { onSave, input } = mount();
  fireEvent.change(input, { target: { value: "Checked" } });
  fireEvent.click(screen.getByRole("button", { name: "Save annotation" }));
  expect(onSave).toHaveBeenCalledWith("Checked", true);
});

it("Escape keeps the annotation and its stored comment, never discarding it", () => {
  const fresh = mount();
  fireEvent.change(fresh.input, { target: { value: "draft" } });
  fireEvent.keyDown(fresh.input, { key: "Escape" });
  expect(fresh.onSave).toHaveBeenCalledWith("", true);
  cleanup();
  const edited = mount({ ...quote, comment: "Earlier" });
  expect(edited.input).toHaveValue("Earlier");
  fireEvent.change(edited.input, { target: { value: "changed" } });
  fireEvent.keyDown(edited.input, { key: "Escape" });
  expect(edited.onSave).toHaveBeenCalledWith("Earlier", true);
});

it("rejects comments beyond 4096 UTF-8 bytes and saves on leaving the field", () => {
  const { onSave, input } = mount();
  fireEvent.change(input, { target: { value: "水".repeat(1366) } });
  expect(input).toHaveValue("");
  expect(screen.getByRole("alert")).toHaveTextContent("4096");
  fireEvent.change(input, { target: { value: "kept" } });
  fireEvent.blur(input, { relatedTarget: document.body });
  expect(onSave).toHaveBeenCalledWith("kept", false);
});

it("saves without stealing focus when its pane loses focus", () => {
  const onSave = vi.fn();
  const content = document.createElement("div");
  const props = { quote, content, scrollport: content, onSave };
  const { rerender } = render(<AnnotationComment {...props} />);
  fireEvent.change(screen.getByRole("textbox"), { target: { value: "x" } });
  rerender(<AnnotationComment {...props} active={false} />);
  expect(onSave).toHaveBeenCalledWith("x", false);
});
