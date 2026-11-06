import { act, cleanup, render, screen } from "@testing-library/react";
import { afterEach, expect, it } from "vitest";
import { useNavigationFocusCarry } from "./useNavigationFocusCarry";

afterEach(cleanup);

// The folded rail loads lazily: right after a fold only its empty fallback is
// mounted, and its buttons arrive in a later commit.
function Rail({ collapsed, loaded }: { collapsed: boolean; loaded: boolean }) {
  useNavigationFocusCarry(collapsed);
  return (
    <>
      {collapsed ? (
        <aside className="project-sidebar" key="icons">
          {loaded ? <button aria-label="Home" /> : null}
        </aside>
      ) : (
        <aside className="project-sidebar" key="full">
          <button aria-label="Home" />
          <button>New chat</button>
        </aside>
      )}
      <textarea aria-label="Composer" />
    </>
  );
}

it("restores rail focus once a lazily loaded rail mounts its controls", async () => {
  const { rerender } = render(<Rail collapsed={false} loaded={false} />);
  screen.getByRole("button", { name: "Home" }).focus();
  rerender(<Rail collapsed loaded={false} />);
  expect(document.body).toHaveFocus();
  await act(async () => rerender(<Rail collapsed loaded />));
  expect(screen.getByRole("button", { name: "Home" })).toHaveFocus();
});

it("leaves focus alone when it moved elsewhere before the rail loaded", async () => {
  const { rerender } = render(<Rail collapsed={false} loaded={false} />);
  screen.getByRole("button", { name: "Home" }).focus();
  rerender(<Rail collapsed loaded={false} />);
  const composer = screen.getByRole("textbox", { name: "Composer" });
  composer.focus();
  await act(async () => rerender(<Rail collapsed loaded />));
  expect(composer).toHaveFocus();
});
