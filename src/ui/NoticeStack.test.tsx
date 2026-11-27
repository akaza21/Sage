import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, expect, it } from "vitest";
import { NoticeStack, type StackEntry } from "./NoticeStack";

afterEach(cleanup);

const line = (key: string, severity: "info" | "warning" | "blocking") => ({
  key,
  severity,
  node: <p>{key}</p>,
});

it("orders blocking, then warning, then info, each in source order", () => {
  render(
    <NoticeStack
      label="Notices"
      notices={[
        line("info a", "info"),
        null,
        line("blocking a", "blocking"),
        line("warning a", "warning"),
        false,
        line("info b", "info"),
        line("blocking b", "blocking"),
      ]}
    />,
  );
  const stack = screen.getByRole("region", { name: "Notices" });
  expect([...stack.children].map((child) => child.textContent)).toEqual([
    "blocking a",
    "blocking b",
    "warning a",
    "info a",
    "info b",
  ]);
  expect(stack).toHaveAttribute("tabindex", "0");
});

it("keeps a quiet live region mounted without making an empty region, and renders nothing when empty", () => {
  const live = (text: string): StackEntry => ({
    key: "live",
    severity: "info",
    quiet: !text,
    node: <div aria-live="polite">{text}</div>,
  });
  const { container, rerender } = render(
    <NoticeStack label="Notices" notices={[live("")]} />,
  );
  const region = container.querySelector("[aria-live=polite]");
  expect(region).not.toBeNull();
  expect(screen.queryByRole("region")).toBeNull();
  expect(container.querySelector("[tabindex]")).toBeNull();
  rerender(<NoticeStack label="Notices" notices={[live("Attached.")]} />);
  // The same live region now holds the copy, so it is announced in place.
  expect(container.querySelector("[aria-live=polite]")).toBe(region);
  expect(screen.getByRole("region", { name: "Notices" })).toContainElement(
    region as HTMLElement,
  );
  rerender(<NoticeStack label="Notices" notices={[null]} />);
  expect(container).toBeEmptyDOMElement();
});
