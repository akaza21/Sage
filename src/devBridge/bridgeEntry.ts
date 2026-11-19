// Entry for a browser tab paired with a running debug Sage (served only by
// the Vite dev server through bridge.html). The app graph evaluates only
// after the bridge has installed the Tauri IPC shim, so runtime detection
// selects the native workbench ports rather than the browser simulator.
const element = document.getElementById("root");

if (!element) {
  throw new Error("Sage could not find its application root.");
}
const root = element;

// Remove the single-use pairing code from the address before any other
// module evaluates.
const pairingCode = location.hash.slice(1);
history.replaceState(history.state, "", location.pathname + location.search);

async function start() {
  const { installBrowserBridge } = await import("./installBrowserBridge");
  await installBrowserBridge(pairingCode);
  const { installWorkbenchNavigationGuard } =
    await import("../app/installWorkbenchNavigationGuard");
  const dispose = installWorkbenchNavigationGuard();
  import.meta.hot?.dispose(dispose);
  const { mountApp } = await import("../app/mountApp");
  mountApp(root);
}

void start().catch((error: unknown) => {
  root.style.color = "#ddd";
  root.textContent =
    error instanceof Error ? error.message : "Sage could not start.";
});
