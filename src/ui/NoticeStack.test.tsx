import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, expect, it } from "vitest";
import { Notice } from "./Notice";
import { NoticeStack, Stacked } from "./NoticeStack";

afterEach(cleanup);

const notice = (identity: string, severity: "warning" | "blocking") => (
  <Notice
    key={identity}
    identity={identity}
    severity={severity}
    message={identity}
  />
);

it("orders blocking, then warning, then info, each in source order", () => {
  render(
    <NoticeStack
      label="Notices"
      notices={[
        <Stacked key="info a">
          <p>info a</p>
        </Stacked>,
        null,
        notice("blocking a", "blocking"),
        notice("warning a", "warning"),
        false,
        <Stacked key="info b" severity="info">
          <p>info b</p>
        </Stacked>,
        notice("blocking b", "blocking"),
        <Stacked key="sign-in" severity="blocking">
          <p>blocking c</p>
        </Stacked>,
      ]}
    />,
  );
  const stack = screen.getByRole("region", { name: "Notices" });
  expect([...stack.children].map((child) => child.textContent)).toEqual([
    "Error: blocking a",
    "Error: blocking b",
    "blocking c",
    "Warning: warning a",
    "info a",
    "info b",
  ]);
  expect(stack).toHaveAttribute("tabindex", "0");
});

it("keeps a quiet live region mounted without making an empty region, and renders nothing when empty", () => {
  const live = (text: string) => (
    <Stacked key="live" quiet={!text}>
      <div aria-live="polite">{text}</div>
    </Stacked>
  );
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
