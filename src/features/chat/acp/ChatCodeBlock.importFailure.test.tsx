import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import { ChatCodeBlock } from "./ChatCodeBlock";

const failedImport = vi.hoisted(() =>
  vi.fn(() => {
    throw new Error("Synthetic lazy module rejection");
  }),
);
vi.mock("../../files/CodeMirrorEditorRuntime", () => failedImport());
afterEach(cleanup);

it("keeps source, copy and wrap usable when the lazy syntax module import rejects", async () => {
  const source = "export const example = 2;";
  const writeText = vi.fn().mockResolvedValue(undefined);
  Object.defineProperty(navigator, "clipboard", {
    configurable: true,
    value: { writeText },
  });
  const { container } = render(<ChatCodeBlock source={source} language="ts" />);
  await act(async () => vi.dynamicImportSettled());
  expect(failedImport).toHaveBeenCalled();
  expect(container.querySelector("code")?.textContent).toBe(source);
  expect(container.querySelector("code span")).toBeNull();
  fireEvent.click(screen.getByRole("button", { name: "Wrap code" }));
  expect(screen.getByRole("button", { name: "Wrap code" })).toHaveAttribute(
    "aria-pressed",
    "true",
  );
  fireEvent.click(screen.getByRole("button", { name: "Copy code" }));
  await waitFor(() =>
    expect(screen.getByRole("status")).toHaveTextContent("Copied"),
  );
  expect(writeText).toHaveBeenCalledWith(source);
});
