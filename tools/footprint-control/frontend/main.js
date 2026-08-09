const states = new Set([
  "shell",
  "chat",
  "file",
  "diff",
  "explorer",
  "source-control",
]);
const app = document.querySelector("#app");
const stateButtons = [...document.querySelectorAll("[data-state]")];

function rows(count, prefix, className = "mock-row") {
  return Array.from(
    { length: count },
    (_, index) =>
      `<div class="${className}"><span>&rsaquo;</span><span>${prefix} ${index + 1}</span></div>`,
  ).join("");
}

function center(state, activeTab) {
  if (activeTab === "secondary") {
    return `<section class="mock-surface" aria-label="Control secondary panel"><h1>Secondary tab</h1><p>Stable tab-switch target.</p></section>`;
  }
  if (state === "chat") {
    return `<section class="mock-surface mock-chat" aria-label="Footprint control chat"><h1>Agent chat</h1>${rows(14, "Message")}<textarea aria-label="Control chat composer"></textarea></section>`;
  }
  if (state === "file") {
    return `<section class="mock-surface" aria-label="Footprint control file"><h1>File editor</h1><textarea class="mock-editor" aria-label="Control file editor">const bounded = true;</textarea></section>`;
  }
  if (state === "diff") {
    return `<section class="mock-surface" aria-label="Footprint control diff"><h1>Unified file diff</h1>${rows(80, "Changed line", "mock-diff-row")}</section>`;
  }
  return `<section class="mock-surface mock-empty" aria-label="Footprint control ${state}"><h1>${state === "shell" ? "Empty workbench" : "Active workbench"}</h1><p>Matched Tauri and WebView2 rendering floor.</p></section>`;
}

function right(state) {
  if (state === "explorer") {
    return `<section class="mock-sidebar" aria-label="Footprint control explorer"><h2>Explorer</h2>${rows(80, "Resource")}</section>`;
  }
  if (state === "source-control") {
    return `<section class="mock-sidebar" aria-label="Footprint control source control"><h2>Source Control</h2>${rows(80, "Change")}</section>`;
  }
  return `<section class="mock-sidebar mock-sidebar--empty" aria-label="Footprint control project tools"><h2>Project tools</h2></section>`;
}

function markup(state, activeTab) {
  return `
    <main class="mock-workbench" aria-label="Footprint control ${state} state">
      <header class="mock-title"><strong>Sage control</strong><span>${state}</span></header>
      <section class="mock-body">
        <aside class="mock-left" aria-label="Control projects and chats">
          <strong>Projects and chats</strong>${rows(40, "Chat")}
        </aside>
        <section class="mock-center">
          <nav class="mock-tabs" aria-label="Control open tabs">
            <button type="button" role="tab" data-tab="primary" aria-selected="${activeTab === "primary"}">Control primary tab</button>
            <button type="button" role="tab" data-tab="secondary" aria-selected="${activeTab === "secondary"}">Control secondary tab</button>
          </nav>
          ${center(state, activeTab)}
          <label class="probe">Interaction probe<input aria-label="Footprint interaction probe" autocomplete="off" /></label>
        </section>
        <aside class="mock-right">${right(state)}<nav class="mock-rail" aria-label="Control activity">${rows(5, "Tool")}</nav></aside>
      </section>
      <footer class="mock-status"><span>main</span><span>Trusted</span></footer>
    </main>`;
}

let currentState = "shell";
let currentTab = "primary";
let sidebarOpen = true;

function mount() {
  app.innerHTML = markup(currentState, currentTab);
  const right = app.querySelector(".mock-right");
  right.hidden = !sidebarOpen;
  const toggle = document.createElement("button");
  toggle.type = "button";
  toggle.className = "sidebar-toggle";
  toggle.setAttribute(
    "aria-label",
    sidebarOpen ? "Hide right sidebar" : "Show right sidebar",
  );
  toggle.textContent = sidebarOpen ? "Hide tools" : "Show tools";
  app.querySelector(".mock-title").append(toggle);
  toggle.addEventListener("click", () => {
    sidebarOpen = !sidebarOpen;
    mount();
  });
  for (const button of stateButtons) {
    button.setAttribute(
      "aria-pressed",
      String(button.dataset.state === currentState),
    );
  }
  for (const button of app.querySelectorAll("[data-tab]")) {
    button.addEventListener("click", () => {
      currentTab = button.dataset.tab;
      mount();
    });
  }
}

for (const button of stateButtons) {
  button.addEventListener("click", () => {
    if (!states.has(button.dataset.state)) return;
    currentState = button.dataset.state;
    currentTab = "primary";
    sidebarOpen = true;
    mount();
  });
}

mount();
