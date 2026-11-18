const element = document.getElementById("root");

if (!element) {
  throw new Error("Sage could not find its application root.");
}
const root = element;

async function mount() {
  if (import.meta.env.DEV) {
    const query = new URLSearchParams(location.search);
    if (query.has("bridge-host")) {
      root.style.cssText = "color: #ddd; padding: 24px; font: 14px system-ui";
      root.textContent = "Sage is open in your browser.";
      return;
    }
    if (query.has("bridge")) {
      const pairingCode = location.hash.slice(1);
      history.replaceState(
        history.state,
        "",
        location.pathname + location.search,
      );
      const { installBrowserBridge } =
        await import("./devBridge/installBrowserBridge");
      await installBrowserBridge(pairingCode);
      const { installWorkbenchNavigationGuard } =
        await import("./app/installWorkbenchNavigationGuard");
      const dispose = installWorkbenchNavigationGuard();
      import.meta.hot?.dispose(dispose);
    }
  }
  await import("./main.desktop");
}

void mount().catch((error: unknown) => {
  root.style.color = "#ddd";
  root.textContent =
    error instanceof Error ? error.message : "Sage could not start.";
});
