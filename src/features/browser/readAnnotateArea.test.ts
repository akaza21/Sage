import { afterEach, beforeEach, expect, it, vi } from "vitest";
import source from "./readAnnotateArea.js?raw";

const rect = {
  x: 0,
  y: 0,
  left: 0,
  top: 0,
  right: 300,
  bottom: 200,
  width: 300,
  height: 200,
  toJSON() {},
};
const read = new Function(
  `return (${source.replace("export default ", "")})`,
)();

beforeEach(() => {
  vi.stubGlobal("visualViewport", {
    pageLeft: 0,
    pageTop: 0,
    offsetLeft: 0,
    offsetTop: 0,
    width: 800,
    height: 600,
    scale: 1,
  });
  vi.spyOn(Element.prototype, "getBoundingClientRect").mockReturnValue(rect);
  vi.spyOn(document, "createRange").mockReturnValue({
    selectNodeContents() {},
    getBoundingClientRect: () => rect,
  } as unknown as Range);
  vi.spyOn(window, "getComputedStyle").mockImplementation(
    (element) =>
      ({
        display: (element as HTMLElement).style.display || "block",
        visibility: (element as HTMLElement).style.visibility || "visible",
        opacity: "1",
        fontSize: (element as HTMLElement).style.fontSize || "16px",
        color: (element as HTMLElement).style.color || "rgb(0, 0, 0)",
        webkitTextFillColor:
          (element as HTMLElement).style.webkitTextFillColor || "rgb(0, 0, 0)",
      }) as CSSStyleDeclaration,
  );
});

it("excludes transparent, faint and tiny text from the area summary", () => {
  document.body.innerHTML =
    '<p>Visible</p><p style="color:transparent">TRANSPARENT_SECRET</p><p style="color:rgba(0,0,0,0.01)">FAINT_SECRET</p><p style="font-size:1px">TINY_SECRET</p><p style="-webkit-text-fill-color:transparent">FILL_SECRET</p>';
  const captured = read({ x: 0, y: 0, width: 300, height: 200 });
  expect(captured.nodes).toContainEqual({ role: "paragraph", text: "Visible" });
  expect(JSON.stringify(captured)).not.toContain("SECRET");
});

it("refuses picture coverage for scroll-driven or excessive animations", () => {
  document.body.innerHTML = "<p>Visible</p>";
  const animations = vi.fn(() => [{ timeline: document.timeline }]);
  Object.defineProperty(Element.prototype, "getAnimations", {
    configurable: true,
    value: animations,
  });
  try {
    expect(read({ x: 0, y: 0, width: 300, height: 200 }).motionSafe).toBe(true);
    animations.mockReturnValue([{ timeline: {} as DocumentTimeline }]);
    expect(read({ x: 0, y: 0, width: 300, height: 200 }).motionSafe).toBe(
      false,
    );
    animations.mockReturnValue(
      Array.from({ length: 513 }, () => ({ timeline: document.timeline })),
    );
    expect(read({ x: 0, y: 0, width: 300, height: 200 }).motionSafe).toBe(
      false,
    );
  } finally {
    Reflect.deleteProperty(Element.prototype, "getAnimations");
  }
});
afterEach(() => {
  document.body.replaceChildren();
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});

it("reads visible text and control roles, never form values or hidden injection", () => {
  document.body.innerHTML =
    '<h1>Cart</h1><p>Visible</p><input value="TYPED_SECRET"><input type="password" value="PASSWORD_SECRET"><textarea>TEXTAREA_SECRET</textarea><select><option>SELECT_SECRET</option></select><div contenteditable="true">EDITABLE_SECRET</div><p aria-hidden="true">ARIA_SECRET</p><p style="display:none">HIDDEN_SECRET</p><p style="visibility:hidden">INVISIBLE_SECRET</p>';
  const captured = read({ x: 0, y: 0, width: 300, height: 200 });
  expect(captured.nodes).toContainEqual({ role: "heading", text: "Cart" });
  expect(captured.nodes).toContainEqual({ role: "textbox", text: "" });
  expect(JSON.stringify(captured)).not.toContain("SECRET");
  expect(captured.masks).toHaveLength(1);
});

it("caps summary elements and text and fails closed on excessive masks or walks", () => {
  document.body.innerHTML = `<p>${"x".repeat(2048)}</p>${"<p>item</p>".repeat(100)}`;
  const captured = read({ x: 0, y: 0, width: 300, height: 200 });
  expect(captured.nodes).toHaveLength(64);
  expect(captured.nodes[0].text).toHaveLength(256);
  document.body.innerHTML = Array.from(
    { length: 513 },
    (_, index) => `<input data-mask="${index}">`,
  ).join("");
  vi.mocked(Element.prototype.getBoundingClientRect).mockImplementation(
    function (this: Element) {
      const index = Number(this.getAttribute("data-mask"));
      return {
        ...rect,
        x: index / 10,
        left: index / 10,
        right: index / 10 + 1,
        width: 1,
      };
    },
  );
  expect(read({ x: 0, y: 0, width: 300, height: 200 })).toBeNull();
  vi.mocked(Element.prototype.getBoundingClientRect).mockReturnValue(rect);
  document.body.innerHTML = "<div></div>".repeat(20001);
  expect(read({ x: 0, y: 0, width: 300, height: 200 })).toBeNull();
});

it("masks editable overflow even when its host is outside the selection", () => {
  document.body.innerHTML =
    '<div contenteditable="true"><span>OVERFLOW_SECRET</span></div>';
  vi.mocked(Element.prototype.getBoundingClientRect).mockImplementation(
    function (this: Element) {
      return this.localName === "span"
        ? rect
        : { ...rect, left: -500, right: -200, x: -500 };
    },
  );
  const captured = read({ x: 0, y: 0, width: 300, height: 200 });
  expect(captured.masks).toContainEqual({
    x: 0,
    y: 0,
    width: 300,
    height: 200,
  });
  expect(JSON.stringify(captured)).not.toContain("OVERFLOW_SECRET");
  document.body.innerHTML =
    '<div contenteditable="true">TEXT_OVERFLOW_SECRET</div>';
  expect(read({ x: 0, y: 0, width: 300, height: 200 }).masks).toContainEqual({
    x: 0,
    y: 0,
    width: 300,
    height: 200,
  });
});
