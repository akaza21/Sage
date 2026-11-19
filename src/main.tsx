// Must stay the first import: it removes the pairing code from the address
// before the app graph below evaluates.
import { takePairingCode } from "./devBridge/pairingCode";
import { mountApp } from "./app/mountApp";

const element = document.getElementById("root");

if (!element) {
  throw new Error("Sage could not find its application root.");
}
const root = element;

function mount() {
  if (import.meta.env.DEV) {
    const query = new URLSearchParams(location.search);
    if (query.has("bridge-host")) {
      root.style.cssText = "color: #ddd; padding: 24px; font: 14px system-ui";
      root.textContent = "Sage is open in your browser.";
      return;
    }
    if (query.has("bridge")) {
      void mountBrowserBridge().catch((error: unknown) => {
        root.style.color = "#ddd";
        root.textContent =
          error instanceof Error ? error.message : "Sage could not start.";
      });
      return;
    }
  }
  mountApp(root);
}

async function mountBrowserBridge() {
  if (import.meta.env.DEV) {
    const { installBrowserBridge } =
      await import("./devBridge/installBrowserBridge");
    await installBrowserBridge(takePairingCode());
    const { installWorkbenchNavigationGuard } =
      await import("./app/installWorkbenchNavigationGuard");
    const dispose = installWorkbenchNavigationGuard();
    import.meta.hot?.dispose(dispose);
    mountApp(root);
  }
}

mount();
