import { act, cleanup, renderHook } from "@testing-library/react";
import { afterEach, expect, it } from "vitest";
import { useComposerPopovers } from "./useChatHarnessSelection";

afterEach(cleanup);

it("ignores stale harness dismissals after opening another composer picker", () => {
  const { result } = renderHook(() => useComposerPopovers());
  act(() => result.current.setHarnessOpen(true));
  const staleHarnessChange = result.current.setHarnessOpen;
  act(() => result.current.toggle("effort"));
  act(() => staleHarnessChange(false));
  expect(result.current.active).toBe("effort");
  act(() => result.current.setHarnessOpen(true));
  expect(result.current.active).toBe("harness");
  act(() => result.current.setHarnessOpen(false));
  expect(result.current.active).toBeNull();
});

it("clears a disabled picker without a queued close affecting a later picker", () => {
  const { result, rerender } = renderHook(
    ({ disabled }) => useComposerPopovers(disabled),
    { initialProps: { disabled: false } },
  );
  act(() => result.current.setHarnessOpen(true));
  rerender({ disabled: true });
  expect(result.current.active).toBeNull();
  act(() => result.current.setHarnessOpen(true));
  expect(result.current.active).toBeNull();
  rerender({ disabled: false });
  expect(result.current.active).toBeNull();
  act(() => result.current.toggle("model"));
  expect(result.current.active).toBe("model");
});
