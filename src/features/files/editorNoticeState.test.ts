import { describe, expect, it } from "vitest";

import { failureMessage } from "./editorNoticeState";

describe("failureMessage", () => {
  it.each([
    "filesystem operation failed: Access is denied. (os error 5)",
    "filesystem operation failed: Permission denied (os error 13)",
    "filesystem operation failed: Read-only file system (os error 30)",
  ])("names a refused write plainly: %s", (raw) => {
    expect(failureMessage(new Error(raw))).toBe(
      "Couldn't save: the file is read-only or you don't have permission.",
    );
  });

  it("names a full disk plainly", () => {
    expect(
      failureMessage(
        new Error(
          "filesystem operation failed: There is not enough space on the disk. (os error 112)",
        ),
      ),
    ).toBe("Couldn't save: the disk is full.");
  });

  it("drops the native prefix and OS error number from anything else", () => {
    expect(
      failureMessage(
        new Error(
          "filesystem operation failed: The network path was not found. (os error 53)",
        ),
      ),
    ).toBe("Couldn't save: The network path was not found.");
  });

  it("says why Keep mine failed in the user's terms", () => {
    expect(
      failureMessage(
        new Error("invalid request: files cannot be created inside .git"),
        "keep",
      ),
    ).toBe(
      "Couldn't keep your version: Sage doesn't create files there (inside protected folders such as .git, or through a link).",
    );
    expect(
      failureMessage(
        new Error(
          "resource limit exceeded: project already has the 64-buffer limit",
        ),
        "keep",
      ),
    ).toBe(
      "Couldn't keep your version: too many files have unsaved changes. Save or close some, then try again.",
    );
  });
});
