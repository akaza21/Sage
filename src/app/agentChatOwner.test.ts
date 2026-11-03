import { createElement, type ReactNode } from "react";
import { act, renderHook } from "@testing-library/react";
import { afterEach, expect, it } from "vitest";

import {
  AgentChatOwnerScopeContext,
  clearAgentChatOwner,
  pinAgentChatOwner,
  pinnedAgentChatOwner,
  releaseAgentChatOwner,
  usePinnedAgentChatOwner,
} from "./agentChatOwner";

afterEach(clearAgentChatOwner);

const insideAgent = ({ children }: { children: ReactNode }) =>
  createElement(AgentChatOwnerScopeContext.Provider, { value: true }, children);

it("withdraws the outside pin at once but keeps the Agent body's focus", () => {
  pinAgentChatOwner("chat-a");
  const outside = renderHook(() => usePinnedAgentChatOwner());
  const inside = renderHook(() => usePinnedAgentChatOwner(), {
    wrapper: insideAgent,
  });

  act(() => releaseAgentChatOwner());
  expect(pinnedAgentChatOwner()).toBeNull();
  expect(outside.result.current).toBeNull();
  expect(inside.result.current).toBe("chat-a");

  act(() => pinAgentChatOwner("chat-b"));
  expect(outside.result.current).toBe("chat-b");
  expect(inside.result.current).toBe("chat-b");
});
