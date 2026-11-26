import { act, cleanup, renderHook } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import { useWorkbenchStore } from "../../app/store";
import type { AcpChatPage } from "../../domain/acp";
import {
  harnessSignInEvidence,
  rememberSignInEvidence,
} from "../../services/acpHarnessDesktop";
import { page } from "./acp/acpProjection.fixture";
import { useChatHarnessSelection } from "./useChatHarnessSelection";

const projection = vi.hoisted(() => ({ page: null as AcpChatPage | null }));
vi.mock("./acp", () => ({ useAcpChatProjection: () => projection.page }));
const initial = useWorkbenchStore.getState();
afterEach(() => {
  cleanup();
  projection.page = null;
  harnessSignInEvidence.clear();
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

it("holds sends after a sign-in failure until newer signed-in evidence", () => {
  const refused = page("failed", { kind: "authRequired" });
  projection.page = {
    ...refused,
    sessions: [{ ...refused.sessions[0], bindingId: "binding-1" }],
    turns: [{ ...refused.turns[0], terminalAt: 10 }],
  } as AcpChatPage;
  useWorkbenchStore.setState({
    harnesses: [],
    selectedHarnessId: "pi",
    harnessDiscoveryPhase: "ready",
  });
  const hook = renderHook(() => useChatHarnessSelection("c"));
  expect(hook.result.current.discoveryPhase).toBe("signInRequired");
  // An account read from before the failure does not release it.
  act(() => void rememberSignInEvidence("binding-1", 5, "signedIn"));
  expect(hook.result.current.discoveryPhase).toBe("signInRequired");
  act(() => void rememberSignInEvidence("binding-1", 20, "signedIn"));
  expect(hook.result.current.discoveryPhase).toBe("ready");
});
