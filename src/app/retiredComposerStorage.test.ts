import { afterEach, beforeEach, expect, it, vi } from "vitest";

beforeEach(() => {
  vi.resetModules();
  localStorage.clear();
});

afterEach(() => {
  vi.restoreAllMocks();
  localStorage.clear();
});

it("removes only retired composer favorites once per load", async () => {
  localStorage.setItem("sage:composer:model-favorites:v1", '["old-model"]');
  localStorage.setItem("sage:ui:theme", "dark");
  const remove = vi.spyOn(Storage.prototype, "removeItem");
  await import("./retiredComposerStorage");
  await import("./retiredComposerStorage");
  expect(localStorage.getItem("sage:composer:model-favorites:v1")).toBeNull();
  expect(localStorage.getItem("sage:ui:theme")).toBe("dark");
  expect(remove).toHaveBeenCalledExactlyOnceWith(
    "sage:composer:model-favorites:v1",
  );
});

it.each(["access", "remove"])(
  "continues loading when storage %s is blocked",
  async (operation) => {
    const blocked = () => {
      throw new DOMException("Storage blocked", "SecurityError");
    };
    if (operation === "access") {
      vi.spyOn(window, "localStorage", "get").mockImplementation(blocked);
    } else {
      vi.spyOn(Storage.prototype, "removeItem").mockImplementation(blocked);
    }
    await expect(import("./retiredComposerStorage")).resolves.toBeDefined();
  },
);
