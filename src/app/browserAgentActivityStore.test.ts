import { act, renderHook } from "@testing-library/react";
import { expect, it } from "vitest";

import {
  clearBrowserAgentPresence,
  setBrowserUserControl,
  useBrowserUserControl,
} from "./browserAgentActivityStore";

it("keeps user control per tab, bounded, until hand back or the tab clears", () => {
  const first = renderHook(() => useBrowserUserControl("browser-1"));
  act(() => setBrowserUserControl("browser-1", "chat-1"));
  expect(first.result.current).toBe("chat-1");
  act(() => clearBrowserAgentPresence("browser-1"));
  expect(first.result.current).toBeNull();

  act(() => {
    for (let index = 1; index <= 9; index += 1)
      setBrowserUserControl(`browser-${index}`, "chat-1");
  });
  expect(first.result.current).toBeNull();
  const last = renderHook(() => useBrowserUserControl("browser-9"));
  expect(last.result.current).toBe("chat-1");
  act(() => setBrowserUserControl("browser-9", null));
  expect(last.result.current).toBeNull();
});
