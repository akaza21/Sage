import { cleanup, renderHook, waitFor } from "@testing-library/react";
import { expect, it, vi } from "vitest";
import { useWorkbenchStore } from "../../../app/store";
import { useAcpProjectionStore } from "../../../app/acpProjectionStore";
import { acpDesktop } from "../../../services/acpDesktop";
import {
  harnessSignInEvidence,
  harnessSignInFailures,
} from "../../../services/acpHarnessDesktop";
import { useAcpChatProjection } from "./useAcpChatProjection";
import { page } from "./acpProjection.fixture";

it("retains a restored refusal delivered before the lazy sign-in module loads", async () => {
  const initial = useWorkbenchStore.getState();
  useWorkbenchStore.setState({
    project: { id: "project-1" } as never,
    projectGeneration: 7,
    nativeTransitionEpoch: 3,
    projectTransitioning: false,
    activeTabId: "chat-1",
    tabs: [{ id: "chat-1", kind: "chat" }] as never,
    harnesses: [
      {
        id: "account",
        name: "Account",
        description: "",
        source: "builtIn",
        installState: "available",
        transport: "acp",
        bindingId: "account-binding",
        accountModelDiscovery: true,
      },
    ],
    selectedHarnessId: "account",
    harnessAccount: "managedChatgpt",
    harnessAccountReadStartedAt: 100,
    harnessDiscoveryPhase: "ready",
  });
  const refused = page("failed", { kind: "authRequired" });
  refused.sessions[0].bindingId = "account-binding" as never;
  refused.turns[0].terminalAt = 200;
  vi.spyOn(acpDesktop, "getChatPage").mockResolvedValue(refused);
  const hook = renderHook(() => useAcpChatProjection());
  let unsubscribe: (() => void) | undefined;
  let reset: (() => void) | undefined;
  try {
    await waitFor(() =>
      expect(hook.result.current?.turns[0].state).toBe("failed"),
    );
    // This import must stay delayed until the authoritative page is accepted.
    const signIn = await import("../../harness/harnessSignIn");
    reset = signIn.resetHarnessSignInForTest;
    unsubscribe = signIn.subscribeHarnessSignIn(() => undefined);
    expect(signIn.harnessSignInStatus("account")).toMatchObject({
      state: "signedOut",
      evidenceAt: 200,
    });
    useWorkbenchStore.setState({ harnessAccountReadStartedAt: 300 });
    expect(signIn.harnessSignInStatus("account")).toMatchObject({
      state: "signedIn",
      evidenceAt: 300,
    });
    useWorkbenchStore.setState({
      nativeTransitionEpoch: 4,
      harnessAccountReadStartedAt: 100,
    });
    expect(signIn.harnessSignInStatus("account")).toMatchObject({
      state: "signedIn",
      evidenceAt: 300,
    });
  } finally {
    cleanup();
    unsubscribe?.();
    reset?.();
    vi.restoreAllMocks();
    useAcpProjectionStore.getState().clear();
    useWorkbenchStore.setState(initial, true);
  }
});

it("bounds retained command evidence and keeps the newest refusal for a binding", () => {
  const refuse = (bindingId: string, terminalAt: number) =>
    harnessSignInFailures.command({
      bindingId,
      busy: true,
      error: {
        failure: { kind: "signInRequired", message: "Sign in", terminalAt },
      },
    });
  try {
    for (let binding = 0; binding < 33; binding++)
      refuse(`binding-${binding}`, 100);
    refuse("binding-32", 200);
    refuse("binding-32", 150);
    expect(harnessSignInEvidence.size).toBe(32);
    expect(harnessSignInEvidence.has("binding-0")).toBe(false);
    expect(harnessSignInEvidence.get("binding-32")).toEqual([200, "signedOut"]);
  } finally {
    harnessSignInEvidence.clear();
  }
});
