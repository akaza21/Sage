/* global window */
// The S3 IPC probe set, which the lane evaluates in a page it did not load
// itself (the `javascript:` popup's blank child). `__ipcProbe` starts the
// probes and returns at once; the outcome lands in `window.__ipcResult`.
window.__ipcProbe = (marker) => {
  window.__ipcResult = null;
  const internals = window.__TAURI_INTERNALS__;
  const probes = [];
  const note = (path, outcome, detail) =>
    probes.push({ path, outcome, detail: String(detail ?? "").slice(0, 300) });
  const later = (ms) =>
    new Promise((resolve) => setTimeout(() => resolve(later), ms));
  const settle = async (start, ms) => {
    try {
      const value = await Promise.race([
        Promise.resolve().then(start),
        later(ms),
      ]);
      return value === later
        ? ["unanswered", ""]
        : ["resolved", JSON.stringify(value)];
    } catch (error) {
      return ["rejected", error?.message ?? error];
    }
  };
  const raw = (send) =>
    new Promise((resolve) => {
      let callback = 0;
      let error = 0;
      if (typeof internals?.transformCallback === "function") {
        callback = internals.transformCallback(
          (value) => resolve(["resolved", JSON.stringify(value)]),
          true,
        );
        error = internals.transformCallback(
          (value) => resolve(["rejected", JSON.stringify(value)]),
          true,
        );
      }
      const message = JSON.stringify({
        cmd: "browser_open",
        callback,
        error,
        payload: { url: marker },
        options: {},
      });
      try {
        send(message);
      } catch (thrown) {
        resolve(["rejected", thrown?.message ?? thrown]);
        return;
      }
      setTimeout(() => resolve(["unanswered", ""]), 2000);
    });
  (async () => {
    for (const [command, args] of [
      ["browser_open", { url: marker }],
      ["browser_live_tabs", {}],
      ["plugin:app|version", {}],
    ]) {
      if (typeof internals?.invoke !== "function")
        note(`invoke ${command}`, "no-bridge", typeof internals);
      else
        note(
          `invoke ${command}`,
          ...(await settle(() => internals.invoke(command, args), 2000)),
        );
    }
    if (typeof window.ipc?.postMessage === "function")
      note(
        "ipc.postMessage",
        ...(await raw((message) => window.ipc.postMessage(message))),
      );
    else note("ipc.postMessage", "absent", typeof window.ipc);
    if (typeof window.chrome?.webview?.postMessage === "function")
      note(
        "chrome.webview.postMessage",
        ...(await raw((message) => window.chrome.webview.postMessage(message))),
      );
    else note("chrome.webview.postMessage", "absent", "");
    for (const base of ["http://ipc.localhost", "ipc://localhost"]) {
      try {
        const response = await Promise.race([
          fetch(`${base}/browser_open`, {
            method: "POST",
            body: JSON.stringify({ url: marker }),
            headers: {
              "Content-Type": "application/json",
              "Tauri-Callback": "1",
              "Tauri-Error": "2",
            },
          }),
          later(2000),
        ]);
        if (response === later) note(`fetch ${base}`, "unanswered", "");
        else
          note(
            `fetch ${base}`,
            response.ok ? "resolved" : "rejected",
            `${response.status} ${(await response.text()).slice(0, 200)}`,
          );
      } catch (error) {
        note(`fetch ${base}`, "rejected", error?.message ?? error);
      }
    }
    window.__ipcResult = { bridge: typeof internals, probes };
  })();
  return "started";
};
