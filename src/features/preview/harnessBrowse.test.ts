import { describe, expect, it } from "vitest";

import {
  MAX_HARNESS_BROWSE_HISTORY,
  MAX_HARNESS_BROWSE_SESSIONS,
  MAX_HARNESS_BROWSE_URL_BYTES,
  admitHarnessBrowseUrl,
  applyBrowseIntent,
  browseBack,
  browseForward,
  browseHistoryForChat,
  browseSessionForChat,
  clearBrowseSessions,
  extractAdmittedBrowseUrl,
} from "./harnessBrowse";

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

  it("rejects a foreign session on the same chat and leaves the first URL open", () => {
    clearBrowseSessions();
    expect(
      applyBrowseIntent({
        chatId: "chat:scoped",
        sessionId: "session-1",
        url: "http://127.0.0.1:5173/",
      })?.href,
    ).toBe("http://127.0.0.1:5173/");
    expect(
      applyBrowseIntent({
        chatId: "chat:scoped",
        sessionId: "session-2",
        url: "http://127.0.0.1:4173/",
      }),
    ).toBeNull();
    expect(browseSessionForChat("chat:scoped")?.href).toBe(
      "http://127.0.0.1:5173/",
    );
    clearBrowseSessions();
  });

  it("keeps at most two per-chat browse sessions and replaces the oldest", () => {
    clearBrowseSessions();
    expect(MAX_HARNESS_BROWSE_SESSIONS).toBe(2);
    expect(
      applyBrowseIntent({ chatId: "chat:a", url: "http://127.0.0.1:5173/" })
        ?.href,
    ).toBe("http://127.0.0.1:5173/");
    expect(
      applyBrowseIntent({ chatId: "chat:b", url: "http://127.0.0.1:4173/" })
        ?.chatId,
    ).toBe("chat:b");
    expect(
      applyBrowseIntent({ chatId: "chat:c", url: "http://127.0.0.1:3000/" })
        ?.chatId,
    ).toBe("chat:c");
    expect(browseSessionForChat("chat:a")).toBeNull();
    expect(browseSessionForChat("chat:b")?.href).toBe("http://127.0.0.1:4173/");
    expect(browseSessionForChat("chat:c")?.href).toBe("http://127.0.0.1:3000/");
    expect(
      applyBrowseIntent({ chatId: "chat:c", url: "javascript:alert(1)" }),
    ).toBeNull();
    expect(browseSessionForChat("chat:c")?.href).toBe("http://127.0.0.1:3000/");
    clearBrowseSessions();
  });

  it("walks a bounded admitted history and drops the forward stack", () => {
    clearBrowseSessions();
    expect(MAX_HARNESS_BROWSE_HISTORY).toBe(8);
    applyBrowseIntent({ chatId: "chat:a", url: "http://127.0.0.1:5173/one" });
    applyBrowseIntent({ chatId: "chat:a", url: "http://127.0.0.1:5173/two" });
    expect(
      applyBrowseIntent({ chatId: "chat:a", url: "https://example.com/" }),
    ).toBeNull();
    expect(browseSessionForChat("chat:a")?.href).toBe(
      "http://127.0.0.1:5173/two",
    );
    expect(browseHistoryForChat("chat:a")).toEqual({
      canBack: true,
      canForward: false,
    });
    expect(browseBack("chat:a")?.href).toBe("http://127.0.0.1:5173/one");
    expect(browseHistoryForChat("chat:a")).toEqual({
      canBack: false,
      canForward: true,
    });
    expect(browseForward("chat:a")?.href).toBe("http://127.0.0.1:5173/two");
    browseBack("chat:a");
    applyBrowseIntent({ chatId: "chat:a", url: "http://127.0.0.1:5173/three" });
    expect(browseSessionForChat("chat:a")?.href).toBe(
      "http://127.0.0.1:5173/three",
    );
    expect(browseForward("chat:a")).toBeNull();
    expect(browseHistoryForChat("chat:a")).toEqual({
      canBack: true,
      canForward: false,
    });
    for (let index = 0; index < 10; index += 1) {
      applyBrowseIntent({
        chatId: "chat:a",
        url: `http://127.0.0.1:5173/p${index}`,
      });
    }
    let steps = 0;
    while (browseBack("chat:a")) steps += 1;
    expect(steps).toBe(MAX_HARNESS_BROWSE_HISTORY - 1);
    expect(browseSessionForChat("chat:a")?.href).toBe(
      "http://127.0.0.1:5173/p2",
    );
    clearBrowseSessions();
  });
});
