import { act, cleanup, renderHook } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import { useWorkbenchStore } from "../../app/store";
import { useChatHarnessSelection } from "./useChatHarnessSelection";

vi.mock("./acp", () => ({ useAcpChatProjection: () => null }));
const initial = useWorkbenchStore.getState();
afterEach(() => {
  cleanup();
  useWorkbenchStore.setState(initial, true);
});

it("does not let an older same-harness selection publish after a newer selection", async () => {
  const releases: (() => void)[] = [];
  const selectHarness = vi.fn(
    () => new Promise<void>((resolve) => releases.push(resolve)),
  );
  const publish = vi.fn();
  useWorkbenchStore.setState({
    harnesses: [],
    selectedHarnessId: "pi",
    selectedModelId: null,
    harnessDiscoveryPhase: "checking",
    captureChatOwnership: () => ({
      projectId: "p",
      chatId: "c",
      ownershipEpoch: 0,
      projectGeneration: 0,
      transitionEpoch: 0,
    }),
    setChatHarnessSelection: publish,
    selectHarness,
  });
  const hook = renderHook(() => useChatHarnessSelection("c"));
  let older!: Promise<void>;
  let newer!: Promise<void>;
  act(() => {
    older = hook.result.current.chooseHarness("pi");
    newer = hook.result.current.chooseHarness("pi");
  });
  expect(publish).toHaveBeenCalledTimes(2);
  await act(async () => {
    useWorkbenchStore.setState({ selectedModelId: "new-model" });
    releases[1]();
    await newer;
  });
  expect(publish).toHaveBeenLastCalledWith(
    expect.objectContaining({ chatId: "c" }),
    { harnessId: "pi", modelId: "new-model" },
  );
  await act(async () => {
    releases[0]();
    await older;
  });
  expect(publish).toHaveBeenCalledTimes(3);
});
