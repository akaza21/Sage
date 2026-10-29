import { renderHook } from "@testing-library/react";
import { afterEach, expect, it } from "vitest";

import {
  applyBrowseIntent,
  browseSessionForOwner,
  clearBrowseSessions,
  harnessBrowseOwner,
  type HarnessBrowseOwner,
} from "../features/preview/harnessBrowse";
import { useRetainedHarnessBrowse } from "./useRetainedHarnessBrowse";

function owner(chatId: string): HarnessBrowseOwner {
  return {
    projectId: "project-a",
    projectGeneration: "1",
    transitionEpoch: 1,
    chatId,
    sessionId: `session-${chatId}`,
    runtimeEpoch: "7",
  } as HarnessBrowseOwner;
}

afterEach(() => clearBrowseSessions());

it("keeps a Chat's Preview across a switch to another Chat and back", () => {
  const chats = ["chat-a", "chat-b", "chat-c"];
  const { rerender } = renderHook(
    ({
      current,
      loading,
    }: {
      current: HarnessBrowseOwner | null;
      loading: boolean;
    }) =>
      useRetainedHarnessBrowse(
        current,
        current ? [current.chatId] : [],
        chats,
        loading,
      ),
    {
      initialProps: {
        current: owner("chat-b") as HarnessBrowseOwner | null,
        loading: false,
      },
    },
  );
  expect(
    applyBrowseIntent({
      owner: owner("chat-b"),
      url: "http://127.0.0.1:4173/",
    }),
  ).not.toBeNull();
  // The next Chat's projection is still loading: nothing is released.
  rerender({ current: null, loading: true });
  expect(browseSessionForOwner(owner("chat-b"))).not.toBeNull();
  rerender({ current: owner("chat-a"), loading: false });
  expect(harnessBrowseOwner()?.chatId).toBe("chat-a");
  rerender({ current: owner("chat-b"), loading: false });
  expect(browseSessionForOwner(owner("chat-b"))?.href).toBe(
    "http://127.0.0.1:4173/",
  );
  // Only two recent Chats are retained.
  rerender({ current: owner("chat-a"), loading: false });
  rerender({ current: owner("chat-c"), loading: false });
  expect(browseSessionForOwner(owner("chat-b"))).toBeNull();
});

it("releases every Preview when no Chat owns the centre", () => {
  const { rerender } = renderHook(
    ({ current }: { current: HarnessBrowseOwner | null }) =>
      useRetainedHarnessBrowse(current, [], ["chat-b"], false),
    { initialProps: { current: owner("chat-b") as HarnessBrowseOwner | null } },
  );
  applyBrowseIntent({ owner: owner("chat-b"), url: "http://127.0.0.1:4173/" });
  rerender({ current: null });
  expect(harnessBrowseOwner()).toBeNull();
  expect(browseSessionForOwner(owner("chat-b"))).toBeNull();
});
