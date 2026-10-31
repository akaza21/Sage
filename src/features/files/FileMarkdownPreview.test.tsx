import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, expect, it } from "vitest";
import { FileMarkdownPreview } from "./FileMarkdownPreview";

afterEach(cleanup);

it("makes exact fenced source keyboard reachable in the file preview", () => {
  const source = "  const value = 'long';  ";
  render(<FileMarkdownPreview source={`\`\`\`ts\n${source}\n\`\`\``} />);
  const code = screen.getByRole("region", { name: "ts code" });
  expect(code).toHaveAttribute("tabindex", "0");
  expect(code.textContent).toBe(source);
  code.focus();
  expect(code).toHaveFocus();
});
