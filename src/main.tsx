const root = document.getElementById("root");

if (!root) {
  throw new Error("Sage could not find its application root.");
}

async function mount() {
  if (import.meta.env.DEV) {
    const query = new URLSearchParams(location.search);
    if (query.has("bridge-host")) {
      root!.style.cssText = "color: #ddd; padding: 24px; font: 14px system-ui";
      root!.textContent = "Sage is open in your browser.";
      return;
    }
    if (query.has("bridge")) {
      const { installBrowserBridge } =
        await import("./devBridge/installBrowserBridge");
      await installBrowserBridge();
    }
  }
  await import("./main.desktop");
}

void mount().catch((error: unknown) => {
  root.style.color = "#ddd";
  root.textContent =
    error instanceof Error ? error.message : "Sage could not start.";
});
