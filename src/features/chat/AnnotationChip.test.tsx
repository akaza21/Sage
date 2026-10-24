import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
} from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import { AnnotationChip, SentAnnotations } from "./AnnotationChip";
import { requestAnnotation } from "./transcriptSelection";

afterEach(cleanup);

const quote = {
  kind: "transcriptQuote" as const,
  ordinal: 1,
  sourceTurnId: "t",
  sourceItemId: "i",
  text: "exact quote",
  comment: "Why?",
  startOffset: 0,
  endOffset: 11,
};

it("groups annotations and exposes exact context with edit/delete actions", () => {
  const edit = vi.fn();
  const remove = vi.fn();
  const clear = vi.fn();
  render(
    <AnnotationChip
      quotes={[quote]}
      onEdit={edit}
      onDelete={remove}
      onClear={clear}
    />,
  );
  fireEvent.mouseEnter(
    screen.getByRole("button", { name: "1 annotation" }).parentElement!,
  );
  const card = screen.getByRole("dialog", { name: "Annotations" });
  expect(card).toHaveTextContent("1.");
  expect(card).toHaveTextContent("Selected text:");
  expect(screen.getByText("exact quote")).toBeVisible();
  expect(screen.getByText("User comment:")).toBeVisible();
  expect(screen.getByText("Why?")).toBeVisible();
  fireEvent.click(screen.getByRole("button", { name: "Edit annotation 1" }));
  expect(edit).toHaveBeenCalledWith(quote);
  fireEvent.mouseEnter(
    screen.getByRole("button", { name: "1 annotation" }).parentElement!,
  );
  fireEvent.click(screen.getByRole("button", { name: "Delete annotation 1" }));
  expect(remove).toHaveBeenCalledWith(quote);
  fireEvent.click(
    screen.getByRole("button", { name: "Remove all annotations" }),
  );
  expect(clear).toHaveBeenCalled();
});

it("omits the comment pair when the comment is empty", () => {
  const { comment: _comment, ...bare } = quote;
  render(<AnnotationChip quotes={[bare]} onDelete={vi.fn()} />);
  fireEvent.focus(screen.getByRole("button", { name: "1 annotation" }));
  expect(screen.getByRole("dialog")).toHaveTextContent("exact quote");
  expect(screen.queryByText("User comment:")).toBeNull();
});

it("renders a read-only sent chip with plural labels", () => {
  render(
    <AnnotationChip
      quotes={[quote, { ...quote, ordinal: 2, sourceItemId: "j" }]}
    />,
  );
  fireEvent.focus(screen.getByRole("button", { name: "2 annotations" }));
  expect(screen.getByRole("dialog", { name: "Annotations" })).toBeVisible();
  expect(screen.queryByRole("button", { name: /Edit annotation/ })).toBeNull();
  expect(
    screen.queryByRole("button", { name: /Delete annotation/ }),
  ).toBeNull();
  expect(
    screen.queryByRole("button", { name: "Remove all annotations" }),
  ).toBeNull();
});

it("labels side-chat seeds as selections without a comment editor", () => {
  render(
    <AnnotationChip
      chatId="side"
      selection
      quotes={[quote]}
      onDelete={vi.fn()}
      onClear={vi.fn()}
    />,
  );
  expect(screen.getByRole("button", { name: "1 selection" })).toBeVisible();
  expect(
    screen.getByRole("button", { name: "Remove all selections" }),
  ).toBeVisible();
  act(() => requestAnnotation("show", "side"));
  expect(screen.getByRole("dialog", { name: "Selections" })).toBeVisible();
  expect(screen.queryByRole("button", { name: /Edit annotation/ })).toBeNull();
});

it("closes the card on Escape and returns focus to the chip", () => {
  render(<AnnotationChip quotes={[quote]} onDelete={vi.fn()} />);
  const trigger = screen.getByRole("button", { name: "1 annotation" });
  fireEvent.focus(trigger);
  fireEvent.keyDown(screen.getByRole("dialog"), { key: "Escape" });
  expect(screen.queryByRole("dialog")).toBeNull();
  expect(trigger).toHaveFocus();
});

it("labels a side chat's sent quotes as selections and others as annotations", () => {
  const { unmount } = render(
    <SentAnnotations contexts={[quote]} chatId="side-chat-1" />,
  );
  expect(screen.getByRole("button", { name: "1 selection" })).toBeVisible();
  unmount();
  render(<SentAnnotations contexts={[quote]} chatId="chat-1" />);
  expect(screen.getByRole("button", { name: "1 annotation" })).toBeVisible();
});
