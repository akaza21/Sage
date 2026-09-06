import { describe, expect, it } from "vitest";

import {
  MAX_HARNESS_BROWSE_HISTORY,
  MAX_HARNESS_BROWSE_SESSIONS,
  MAX_HARNESS_BROWSE_URL_BYTES,
  admitHarnessBrowseUrl,
  applyBrowseIntent,
  browseBack,
  browseForward,
  browseHistoryForOwner,
  browseSessionForOwner,
  clearBrowseSession,
  clearBrowseSessions,
  extractAdmittedBrowseUrl,
  setHarnessBrowseOwner,
  type HarnessBrowseOwner,
} from "./harnessBrowse";

function owner(
  chatId: string,
  sessionId = "session-1",
  projectId = "project-1",
): HarnessBrowseOwner {
  return {
    projectId,
    projectGeneration: "1",
    transitionEpoch: 1,
    chatId,
    sessionId,
    runtimeEpoch: "1",
  } as HarnessBrowseOwner;
}

function visible(...owners: HarnessBrowseOwner[]): void {
  setHarnessBrowseOwner(
    owners[0] ?? null,
    owners.map((value) => value.chatId),
  );
}

describe("fail-closed harness browse admission", () => {
  it("admits loopback HTTP(S) with an explicit port and no credentials", () => {
    expect(admitHarnessBrowseUrl("http://127.0.0.1:5173/app")?.href).toBe(
      "http://127.0.0.1:5173/app",
    );
    expect(admitHarnessBrowseUrl("https://[::1]:8443/")?.href).toBe(
      "https://[::1]:8443/",
    );
  });

  it("rejects non-loopback, credentialed, schemed, and oversized URLs", () => {
    expect(admitHarnessBrowseUrl("")).toBeNull();
    expect(admitHarnessBrowseUrl("http://127.0.0.1/app")).toBeNull();
    expect(admitHarnessBrowseUrl("http://localhost:5173/")).toBeNull();
    expect(admitHarnessBrowseUrl("http://example.com:443/")).toBeNull();
    expect(admitHarnessBrowseUrl("https://example.com/docs")).toBeNull();
    expect(
      admitHarnessBrowseUrl("http://user:pass@127.0.0.1:5173/"),
    ).toBeNull();
    expect(admitHarnessBrowseUrl("file:///tmp/index.html")).toBeNull();
    expect(admitHarnessBrowseUrl("javascript:alert(1)")).toBeNull();
    expect(admitHarnessBrowseUrl("data:text/html,hi")).toBeNull();
    expect(
      admitHarnessBrowseUrl(
        `http://127.0.0.1:5173/${"a".repeat(MAX_HARNESS_BROWSE_URL_BYTES)}`,
      ),
    ).toBeNull();
  });

  it("extracts only an admitted loopback URL from bounded tool text", () => {
    expect(
      extractAdmittedBrowseUrl(
        "Opened http://127.0.0.1:4173/preview and https://example.com/x",
      )?.href,
    ).toBe("http://127.0.0.1:4173/preview");
    expect(extractAdmittedBrowseUrl("no url here")).toBeNull();
    expect(extractAdmittedBrowseUrl("see https://example.com")).toBeNull();
  });

  it("retains independent sessions for the same Chat without evicting another Chat", () => {
    clearBrowseSessions();
    const oldOwner = owner("chat:scoped", "session-1");
    const otherOwner = owner("chat:other", "session-1");
    const replacementOwner = owner("chat:scoped", "session-2");
    visible(oldOwner, otherOwner);
    setHarnessBrowseOwner(otherOwner, [oldOwner.chatId, otherOwner.chatId]);
    applyBrowseIntent({
      owner: otherOwner,
      url: "http://127.0.0.1:3000/other",
    });
    setHarnessBrowseOwner(oldOwner, [oldOwner.chatId, otherOwner.chatId]);
    applyBrowseIntent({
      owner: oldOwner,
      url: "http://127.0.0.1:5173/old",
    });
    setHarnessBrowseOwner(replacementOwner, [
      oldOwner.chatId,
      otherOwner.chatId,
    ]);
    expect(
      applyBrowseIntent({
        owner: replacementOwner,
        url: "http://127.0.0.1:4173/new",
      })?.href,
    ).toBe("http://127.0.0.1:4173/new");
    expect(browseSessionForOwner(oldOwner)?.href).toBe(
      "http://127.0.0.1:5173/old",
    );
    expect(browseSessionForOwner(otherOwner)?.href).toBe(
      "http://127.0.0.1:3000/other",
    );
    expect(browseHistoryForOwner(replacementOwner)).toEqual({
      canBack: false,
      canForward: false,
    });
  });

  it("retains only the two visible Chats without global cross-Chat eviction", () => {
    clearBrowseSessions();
    const ownerA = owner("chat:a");
    const ownerB = owner("chat:b");
    const ownerC = owner("chat:c");
    visible(ownerA, ownerB);
    expect(MAX_HARNESS_BROWSE_SESSIONS).toBe(2);
    expect(
      applyBrowseIntent({ owner: ownerA, url: "http://127.0.0.1:5173/" })?.href,
    ).toBe("http://127.0.0.1:5173/");
    setHarnessBrowseOwner(ownerB, [ownerA.chatId, ownerB.chatId]);
    expect(
      applyBrowseIntent({ owner: ownerB, url: "http://127.0.0.1:4173/" })
        ?.owner,
    ).toEqual(ownerB);
    expect(
      applyBrowseIntent({ owner: ownerC, url: "http://127.0.0.1:3000/" }),
    ).toBeNull();
    expect(browseSessionForOwner(ownerA)?.href).toBe("http://127.0.0.1:5173/");
    expect(browseSessionForOwner(ownerB)?.href).toBe("http://127.0.0.1:4173/");
    expect(browseSessionForOwner(ownerC)).toBeNull();
    expect(
      applyBrowseIntent({ owner: ownerC, url: "javascript:alert(1)" }),
    ).toBeNull();
    expect(browseSessionForOwner(ownerC)).toBeNull();
  });

  it("evicts only the deterministic oldest of three sessions in one Chat", () => {
    clearBrowseSessions();
    const first = owner("chat:a", "session-1");
    const second = owner("chat:a", "session-2");
    const third = owner("chat:a", "session-3");
    const isolated = owner("chat:b", "session-1");
    visible(first, isolated);
    applyBrowseIntent({ owner: first, url: "http://127.0.0.1:5173/one" });
    setHarnessBrowseOwner(second, [first.chatId, isolated.chatId]);
    applyBrowseIntent({ owner: second, url: "http://127.0.0.1:5173/two" });
    setHarnessBrowseOwner(isolated, [first.chatId, isolated.chatId]);
    applyBrowseIntent({ owner: isolated, url: "http://127.0.0.1:4173/keep" });
    setHarnessBrowseOwner(third, [first.chatId, isolated.chatId]);
    applyBrowseIntent({ owner: third, url: "http://127.0.0.1:5173/three" });
    expect(browseSessionForOwner(first)).toBeNull();
    expect(browseSessionForOwner(second)?.href).toBe(
      "http://127.0.0.1:5173/two",
    );
    expect(browseSessionForOwner(third)?.href).toBe(
      "http://127.0.0.1:5173/three",
    );
    expect(browseSessionForOwner(isolated)?.href).toBe(
      "http://127.0.0.1:4173/keep",
    );
  });

  it("walks a bounded per-owner history and drops the forward stack", () => {
    clearBrowseSessions();
    const browseOwner = owner("chat:a");
    visible(browseOwner);
    expect(MAX_HARNESS_BROWSE_HISTORY).toBe(8);
    applyBrowseIntent({
      owner: browseOwner,
      url: "http://127.0.0.1:5173/one",
    });
    applyBrowseIntent({
      owner: browseOwner,
      url: "http://127.0.0.1:5173/two",
    });
    expect(
      applyBrowseIntent({ owner: browseOwner, url: "https://example.com/" }),
    ).toBeNull();
    expect(browseSessionForOwner(browseOwner)?.href).toBe(
      "http://127.0.0.1:5173/two",
    );
    expect(browseHistoryForOwner(browseOwner)).toEqual({
      canBack: true,
      canForward: false,
    });
    expect(browseBack(browseOwner)?.href).toBe("http://127.0.0.1:5173/one");
    expect(browseHistoryForOwner(browseOwner)).toEqual({
      canBack: false,
      canForward: true,
    });
    expect(browseForward(browseOwner)?.href).toBe("http://127.0.0.1:5173/two");
    browseBack(browseOwner);
    applyBrowseIntent({
      owner: browseOwner,
      url: "http://127.0.0.1:5173/three",
    });
    expect(browseForward(browseOwner)).toBeNull();
    for (let index = 0; index < 10; index += 1) {
      applyBrowseIntent({
        owner: browseOwner,
        url: `http://127.0.0.1:5173/p${index}`,
      });
    }
    let steps = 0;
    while (browseBack(browseOwner)) steps += 1;
    expect(steps).toBe(MAX_HARNESS_BROWSE_HISTORY - 1);
    expect(browseSessionForOwner(browseOwner)?.href).toBe(
      "http://127.0.0.1:5173/p2",
    );
  });

  it("closes only the exact Project, Chat, and session tuple", () => {
    clearBrowseSessions();
    const ownerA = owner("chat:a");
    const ownerB = owner("chat:b");
    visible(ownerA, ownerB);
    applyBrowseIntent({ owner: ownerA, url: "http://127.0.0.1:5173/" });
    setHarnessBrowseOwner(ownerB, [ownerA.chatId, ownerB.chatId]);
    applyBrowseIntent({ owner: ownerB, url: "http://127.0.0.1:4173/" });
    clearBrowseSession(ownerA);
    expect(browseSessionForOwner(ownerA)).toBeNull();
    expect(browseSessionForOwner(ownerB)?.href).toBe("http://127.0.0.1:4173/");
  });

  it("rejects stale navigation after a six-field owner replacement", () => {
    clearBrowseSessions();
    const oldOwner = owner("chat:a", "session-1");
    const replacement = {
      ...oldOwner,
      sessionId: "session-2",
      runtimeEpoch: "2",
    } as HarnessBrowseOwner;
    visible(oldOwner);
    applyBrowseIntent({
      owner: oldOwner,
      url: "http://127.0.0.1:5173/old",
    });
    setHarnessBrowseOwner(replacement, [replacement.chatId]);
    expect(
      applyBrowseIntent({
        owner: oldOwner,
        url: "http://127.0.0.1:4173/stale",
      }),
    ).toBeNull();
    expect(browseSessionForOwner(oldOwner)?.href).toBe(
      "http://127.0.0.1:5173/old",
    );
  });

  it("clears foreign Project scope sessions and history atomically", () => {
    clearBrowseSessions();
    const previous = owner("chat:a", "session-1", "project:old");
    const replacement = {
      ...owner("chat:a", "session-1", "project:new"),
      projectGeneration: "2",
      transitionEpoch: 2,
    } as HarnessBrowseOwner;
    visible(previous);
    applyBrowseIntent({
      owner: previous,
      url: "http://127.0.0.1:5173/old",
    });
    setHarnessBrowseOwner(replacement, [replacement.chatId]);
    expect(browseSessionForOwner(previous)).toBeNull();
    expect(browseHistoryForOwner(previous)).toEqual({
      canBack: false,
      canForward: false,
    });
  });
});
