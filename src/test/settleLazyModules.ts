import { act } from "@testing-library/react";
import { vi } from "vitest";

const SETTLE_PASSES = 3;

export async function settleLazyModules(): Promise<void> {
  for (let pass = 0; pass < SETTLE_PASSES; pass += 1) {
    await act(async () => {
      await vi.dynamicImportSettled();
    });
  }
}
