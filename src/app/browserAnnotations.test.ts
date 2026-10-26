import { afterEach, expect, it, vi } from "vitest";
import {
  browserElementSink,
  handOffBrowserElement,
  parseBrowserElement,
  registerBrowserElementSink,
  type BrowserElementContext,
} from "./browserAnnotations";

const item: BrowserElementContext = {
  kind: "browserElement",
  target: "element",
  ordinal: 3,
  tabId: "browser-12",
  url: "https://example.com/cart",
  title: "Cart",
  selector: "#checkout > :nth-child(2 of button)",
  role: "button",
  name: "Place order",
  text: "Place order\n£24.00",
  box: { x: 412, y: 1880, width: 160, height: 40 },
  comment: "This should be disabled until terms are ticked",
};
let unregister = () => {};
afterEach(() => unregister());

const rejects = (change: Record<string, unknown>) =>
  expect(parseBrowserElement({ ...item, ...change })).toBeNull();

it("admits the contract item as it is", () => {
  expect(parseBrowserElement(item)).toEqual(item);
});

it("admits each field at its limit and rejects one byte more", () => {
  for (const [field, limit] of [
    ["title", 256],
    ["selector", 512],
    ["name", 256],
    ["text", 1024],
    ["comment", 4096],
  ] as const) {
    const base = { ...item, comment: "" };
    expect(
      parseBrowserElement({ ...base, [field]: "a".repeat(limit) }),
      field,
    ).not.toBeNull();
    expect(
      parseBrowserElement({ ...base, [field]: "a".repeat(limit + 1) }),
      field,
    ).toBeNull();
  }
  // Multi-byte text counts bytes, not characters.
  rejects({ title: "é".repeat(129) });
  rejects({ role: "a".repeat(65) });
  expect(parseBrowserElement({ ...item, role: "a".repeat(64) })).not.toBeNull();
});

it("rejects bidi and control characters, and line breaks outside the text", () => {
  rejects({ title: "a‮b" });
  rejects({ name: "a⁦" });
  rejects({ selector: "div\n> p" });
  rejects({ text: "a\tb" });
  rejects({ text: "a\u0007" });
  expect(parseBrowserElement({ ...item, text: "one\ntwo" })).not.toBeNull();
});

it("rejects unknown keys, other kinds and out-of-range numbers", () => {
  rejects({ extra: 1 });
  rejects({ kind: "browserPage" });
  rejects({ target: "area" });
  for (const ordinal of [0, 9, 1.5]) rejects({ ordinal });
  for (const tabId of ["main", "browser-0", "browser-01"]) rejects({ tabId });
  rejects({ selector: "" });
  rejects({ role: "button role" });
  rejects({ box: { x: -1, y: 0, width: 1, height: 1 } });
  rejects({ box: { x: 0, y: 0, width: 0, height: 1 } });
  rejects({ box: { x: 0, y: 1e7 + 1, width: 1, height: 1 } });
  rejects({ box: { x: 0, y: 0, width: 1, height: 1, z: 0 } });
  const { box: _box, ...noBox } = item;
  expect(parseBrowserElement(noBox)).toBeNull();
  expect(parseBrowserElement(null)).toBeNull();
});

it("rejects URLs that are not plain web URLs", () => {
  for (const url of [
    "javascript:alert(1)",
    "file:///etc/passwd",
    "https://user:pw@example.com/",
    `https://example.com/${"a".repeat(2048)}`,
    "not a url",
  ])
    rejects({ url });
});

it("rejects a whole entry over 8 KiB", () => {
  const full = {
    ...item,
    title: "c".repeat(256),
    selector: "d".repeat(512),
    name: "b".repeat(256),
    text: "a".repeat(1024),
    comment: "e".repeat(4096),
  };
  expect(parseBrowserElement(full)).not.toBeNull();
  rejects({ ...full, url: `https://example.com/${"f/".repeat(1000)}` });
});

it("hands a checked item to the registered sink only", () => {
  expect(handOffBrowserElement(item)).toBe(false);
  const attach = vi.fn();
  const sink = { nextOrdinal: () => 1, attach };
  unregister = registerBrowserElementSink(sink);
  expect(browserElementSink()).toBe(sink);
  expect(handOffBrowserElement({ ...item, url: "file:///x" })).toBe(false);
  expect(attach).not.toHaveBeenCalled();
  expect(handOffBrowserElement(item)).toBe(true);
  expect(attach).toHaveBeenCalledWith(item);
  unregister();
  expect(browserElementSink()).toBeNull();
});

it("keeps a newer sink when an older one unregisters", () => {
  const older = registerBrowserElementSink({
    nextOrdinal: () => 1,
    attach: vi.fn(),
  });
  const newer = { nextOrdinal: () => 2, attach: vi.fn() };
  unregister = registerBrowserElementSink(newer);
  older();
  expect(browserElementSink()).toBe(newer);
});
