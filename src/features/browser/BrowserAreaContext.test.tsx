import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import { parseBrowserElementContext } from "../../domain/attachedContext";
import BrowserElementChips, {
  browserElementLabel,
} from "./BrowserElementChips";
import { browserCropPreview, rememberBrowserCrop } from "./browserCropPreview";

afterEach(cleanup);

const area = {
  kind: "browserElement",
  target: "area",
  ordinal: 3,
  tabId: "browser-1",
  url: "https://example.com/cart",
  title: "Cart",
  text: "heading: Cart",
  box: { x: 10, y: 20, width: 300, height: 200 },
  comment: "Simplify this area",
};

it("admits areas but rejects element-only fields and crop egress", () => {
  expect(parseBrowserElementContext(area)).toEqual(area);
  for (const extra of [
    { selector: "" },
    { role: "button" },
    { name: "" },
    { crop: {} },
  ])
    expect(parseBrowserElementContext({ ...area, ...extra })).toBeNull();
  expect(
    parseBrowserElementContext({ ...area, text: "x".repeat(1024) }),
  ).not.toBeNull();
  expect(
    parseBrowserElementContext({ ...area, text: "x".repeat(1025) }),
  ).toBeNull();
});

it("shows the shared removable area chip and its URL-only tooltip", () => {
  const item = parseBrowserElementContext(area)!;
  expect(item).not.toBeNull();
  expect(browserElementLabel(item)).toBe("Area 3 · example.com");
  render(
    <ul>
      <BrowserElementChips contexts={[item]} onRemove={() => {}} />
    </ul>,
  );
  const chip = screen.getByRole("button", { name: "Area 3 · example.com" });
  expect(chip).toHaveAttribute("title", area.url);
  fireEvent.focus(chip);
  expect(screen.getByRole("dialog")).toHaveTextContent("Simplify this area");
  expect(
    screen.getByRole("button", { name: "Remove Area 3 · example.com" }),
  ).toBeEnabled();
});

it("removes a masked picture independently of its area context", () => {
  const item = parseBrowserElementContext(area)!;
  rememberBrowserCrop(item, {
    width: 300,
    height: 200,
    pngBase64: "iVBORw0KGgo=",
  });
  const remove = vi.fn();
  render(
    <ul>
      <BrowserElementChips contexts={[item]} onRemove={remove} />
    </ul>,
  );
  fireEvent.focus(screen.getByRole("button", { name: "Area 3 · example.com" }));
  expect(
    screen.getByRole("img", { name: "Masked area preview" }),
  ).toBeInTheDocument();
  fireEvent.click(
    screen.getByRole("button", { name: "Remove picture from Area 3" }),
  );
  expect(screen.queryByRole("img")).toBeNull();
  expect(remove).not.toHaveBeenCalled();
  expect(browserCropPreview(item)).toBeUndefined();
});

it("keeps at most eight disposable picture previews", () => {
  const entries = Array.from({ length: 9 }, () =>
    parseBrowserElementContext({ ...area })!,
  );
  for (const entry of entries)
    rememberBrowserCrop(entry, {
      width: 1,
      height: 1,
      pngBase64: "iVBORw0KGgo=",
    });
  expect(browserCropPreview(entries[0]!)).toBeUndefined();
  expect(entries.slice(1).every((entry) => browserCropPreview(entry))).toBe(
    true,
  );
});
