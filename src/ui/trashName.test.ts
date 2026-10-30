import { afterEach, describe, expect, it, vi } from "vitest";

import { trashName } from "./trashName";

function onPlatform(platform: string, userAgent = "") {
  vi.spyOn(navigator, "platform", "get").mockReturnValue(platform);
  vi.spyOn(navigator, "userAgent", "get").mockReturnValue(userAgent);
}

describe("trashName", () => {
  afterEach(() => vi.restoreAllMocks());

  it("says Recycle Bin on Windows", () => {
    onPlatform("Win32");
    expect(trashName()).toBe("Recycle Bin");
    onPlatform("", "Mozilla/5.0 (Windows NT 10.0; Win64; x64)");
    expect(trashName()).toBe("Recycle Bin");
  });

  it("says Trash on macOS and Linux, including a Darwin user agent", () => {
    onPlatform("MacIntel");
    expect(trashName()).toBe("Trash");
    onPlatform("", "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) Darwin");
    expect(trashName()).toBe("Trash");
    onPlatform("Linux x86_64");
    expect(trashName()).toBe("Trash");
  });
});
