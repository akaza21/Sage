// Shown by the dev server when a plain browser tab has no Sage backend: no
// desktop shell, no dev bridge, and not the Playwright `ui-fixture` mode.
// Plain DOM with inline style properties, so it needs no app stylesheet.
export function renderBackendMissing(root: HTMLElement): void {
  const panel = document.createElement("main");
  panel.setAttribute("role", "alert");
  Object.assign(panel.style, {
    maxWidth: "32rem",
    margin: "20vh auto 0",
    padding: "0 16px",
    color: "#d8d8d8",
    font: "14px/1.5 system-ui, sans-serif",
  });
  const title = document.createElement("h1");
  title.textContent = "Sage's backend isn't running";
  Object.assign(title.style, { fontSize: "18px", fontWeight: "600" });
  const detail = document.createElement("p");
  const command = document.createElement("code");
  command.textContent = "npm run dev";
  detail.append("Start it with ", command, " and open the URL it prints.");
  panel.append(title, detail);
  root.replaceChildren(panel);
}
