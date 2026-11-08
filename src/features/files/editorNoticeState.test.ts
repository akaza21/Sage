import { describe, expect, it } from "vitest";

import { saveFailureMessage } from "./editorNoticeState";

describe("saveFailureMessage", () => {
  it.each([
    "filesystem operation failed: Access is denied. (os error 5)",
    "filesystem operation failed: Permission denied (os error 13)",
    "filesystem operation failed: Read-only file system (os error 30)",
  ])("names a refused write plainly: %s", (raw) => {
    expect(saveFailureMessage(new Error(raw))).toBe(
      "Couldn't save: the file is read-only or you don't have permission.",
    );
  });

  it("names a full disk plainly", () => {
    expect(
      saveFailureMessage(
        new Error(
          "filesystem operation failed: There is not enough space on the disk. (os error 112)",
        ),
      ),
    ).toBe("Couldn't save: the disk is full.");
  });

  it("drops the native prefix and OS error number from anything else", () => {
    expect(
      saveFailureMessage(
        new Error(
          "filesystem operation failed: The network path was not found. (os error 53)",
        ),
      ),
    ).toBe("Couldn't save: The network path was not found.");
  });
});
