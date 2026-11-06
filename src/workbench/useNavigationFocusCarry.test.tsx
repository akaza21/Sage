import { act, cleanup, render, screen } from "@testing-library/react";
import { afterEach, expect, it } from "vitest";
import { useNavigationFocusCarry } from "./useNavigationFocusCarry";

afterEach(cleanup);

// The folded rail loads lazily: right after a fold only its empty fallback is
// mounted, and the rail itself arrives in a later commit.
function Rail({ collapsed, loaded }: { collapsed: boolean; loaded: boolean }) {
  const railRef = useNavigationFocusCarry(collapsed);
  return (
    <>
      {!collapsed ? (
        <aside className="project-sidebar" key="full" ref={railRef}>
          <button aria-label="Home" />
          <button>New chat</button>
        </aside>
      ) : loaded ? (
        <aside className="project-sidebar" key="icons" ref={railRef}>
          <button aria-label="Search" />
          <button aria-label="Home" />
        </aside>
      ) : (
        <div className="project-sidebar" key="fallback" />
      )}
      <textarea aria-label="Composer" />
      <p>Plain text</p>
    </>
  );
}

function foldFrom(name: string) {
  const view = render(<Rail collapsed={false} loaded={false} />);
  screen.getByRole("button", { name }).focus();
  view.rerender(<Rail collapsed loaded={false} />);
  expect(document.body).toHaveFocus();
  return view;
}

it("restores rail focus once a lazily loaded rail mounts", async () => {
  const { rerender } = foldFrom("Home");
  await act(async () => rerender(<Rail collapsed loaded />));
  expect(screen.getByRole("button", { name: "Home" })).toHaveFocus();
});

it("falls back to the new rail's first control when the name is gone", async () => {
  const { rerender } = foldFrom("New chat");
  await act(async () => rerender(<Rail collapsed loaded />));
  expect(screen.getByRole("button", { name: "Search" })).toHaveFocus();
});

it("restores immediately when the new rail mounts in the same commit", () => {
  const { rerender } = render(<Rail collapsed loaded />);
  screen.getByRole("button", { name: "Home" }).focus();
  rerender(<Rail collapsed={false} loaded />);
  expect(screen.getByRole("button", { name: "Home" })).toHaveFocus();
});

it("never steals focus that left and returned to the body before loading", async () => {
  const { rerender } = foldFrom("Home");
  const composer = screen.getByRole("textbox", { name: "Composer" });
  composer.focus();
  composer.blur();
  expect(document.body).toHaveFocus();
  await act(async () => rerender(<Rail collapsed loaded />));
  expect(document.body).toHaveFocus();
});
