import { JSDOM } from "jsdom";
import {
  EDITOR_FILE,
  STATE_WITNESSES,
} from "./capture-native-editor-catalog.mjs";
import { observeEditor } from "./capture-native-editor-dom.mjs";
import { visibleRectangle } from "./capture-native-chat-dom.mjs";

export function editorDom(state = "editor-idle", width = 1888) {
  const dom = new JSDOM(
    `<input type="radio" name="workbench-window-mode" value="editor" checked>
    <nav class="activity-rail"><button aria-label="Explorer" aria-expanded="true"></button><button aria-label="Outline" aria-expanded="false"></button><button aria-label="Search" aria-expanded="false"></button></nav>
    <section aria-label="Explorer project tools"><button role="treeitem" aria-label="Toggle folder src" aria-expanded="false"></button><button role="treeitem" aria-label="Toggle folder src/features" aria-expanded="false"></button><button role="treeitem" aria-label="Toggle folder src/features/outline" aria-expanded="false"></button><button role="treeitem" aria-label="Open file ${EDITOR_FILE}"></button></section>
    <div class="center-region"><div class="workbench__tabs"><button role="tab" aria-selected="true">syntaxOutline.ts</button></div>
    <section class="file-surface" aria-label="File editor ${EDITOR_FILE}"><div class="cm-content" contenteditable="true" tabindex="0">export function sample() {}</div></section></div>
    <span title="Caret position">Ln 50, Col 1</span><button aria-label="Open settings"></button>`,
    { runScripts: "outside-only", pretendToBeVisual: true },
  );
  const { window } = dom,
    { document } = window;
  Object.defineProperties(window, {
    innerWidth: { value: width },
    innerHeight: { value: width === 1888 ? 1096 : 1000 },
    devicePixelRatio: { value: 1, configurable: true },
  });
  window.HTMLElement.prototype.getBoundingClientRect = () => ({
    x: 5,
    y: 5,
    width: 180,
    height: 24,
  });
  window.HTMLElement.prototype.scrollIntoView = () => {};
  const context = {
    state,
    file: EDITOR_FILE,
    midLine: 50,
    witnesses: STATE_WITNESSES[state],
  };
  const caret = document.querySelector('[title="Caret position"]');
  const content = document.querySelector(".cm-content");
  const setCaret = (line, column = 1) => {
    caret.textContent = `Ln ${line}, Col ${column}`;
  };
  const focusContent = () => {
    content.focus();
    const range = document.createRange();
    range.setStart(content.firstChild, 0);
    range.collapse(true);
    const selection = window.getSelection();
    selection.removeAllRanges();
    selection.addRange(range);
  };
  const openGoto = () => {
    const prompt = document.createElement("div");
    prompt.setAttribute("role", "search");
    prompt.setAttribute("aria-label", "Go to line");
    prompt.innerHTML =
      '<input aria-label="Go to line"><span role="status">Line or line:column</span>';
    document.querySelector(".file-surface").append(prompt);
    const input = prompt.querySelector("input");
    input.addEventListener("keydown", (event) => {
      if (event.key === "Enter") {
        setCaret(Number(input.value));
        prompt.remove();
        focusContent();
      }
    });
    input.focus();
    return input;
  };
  const openOutline = () => {
    const section = document.createElement("section");
    section.setAttribute("aria-label", "Outline");
    section.innerHTML = `<button class="outline-card" aria-label="Function: sample. ${EDITOR_FILE}, line 20, column 1">sample</button>`;
    document.body.append(section);
    document
      .querySelector('[aria-label="Outline"]')
      .setAttribute("aria-expanded", "true");
    const symbol = section.querySelector("button");
    symbol.addEventListener("click", () => {
      setCaret(20);
      focusContent();
    });
    return symbol;
  };
  const openSearch = () => {
    const section = document.createElement("section");
    section.className = "workspace-search";
    section.innerHTML =
      '<input aria-label="Search text">' +
      [EDITOR_FILE, "src/features/outline/outlineCard.ts"]
        .map(
          (file) =>
            `<section class="workspace-search__file-group"><button class="workspace-search__file-heading" aria-label="Collapse results for ${file}" aria-expanded="true"></button><button class="workspace-search__result">export</button></section>`,
        )
        .join("");
    document.body.append(section);
    document
      .querySelector('[aria-label="Search"]')
      .setAttribute("aria-expanded", "true");
    return section.querySelector("input");
  };
  const openSettings = () => {
    document
      .querySelector('.workbench__tabs [role="tab"]')
      .setAttribute("aria-selected", "false");
    document
      .querySelector(".workbench__tabs")
      .insertAdjacentHTML(
        "beforeend",
        '<button role="tab" aria-selected="true">Settings</button>',
      );
    document.body.insertAdjacentHTML(
      "beforeend",
      '<section class="settings-surface" aria-label="Settings" aria-busy="false"><nav class="settings-surface__nav"><button role="tab" aria-selected="true">Editor</button></nav><section class="settings-surface__group"><input id="editor-font-size" value="13"></section></section>',
    );
  };
  for (const folder of document.querySelectorAll(
    '[aria-label^="Toggle folder"]',
  ))
    folder.addEventListener("click", () =>
      folder.setAttribute("aria-expanded", "true"),
    );
  document
    .querySelector('.activity-rail [aria-label="Outline"]')
    .addEventListener("click", openOutline);
  document
    .querySelector('.activity-rail [aria-label="Search"]')
    .addEventListener("click", openSearch);
  document
    .querySelector('[aria-label="Open settings"]')
    .addEventListener("click", openSettings);
  if (width < 761) {
    const center = document.querySelector(".center-region");
    center.setAttribute("inert", "");
    center.setAttribute("aria-hidden", "true");
    const explorer = document.querySelector(
      '[aria-label="Explorer project tools"]',
    );
    explorer.addEventListener("keydown", (event) => {
      if (event.key === "Escape") {
        explorer.remove();
        center.removeAttribute("inert");
        center.removeAttribute("aria-hidden");
      }
    });
    document
      .querySelector('.activity-rail [aria-label="Outline"]')
      .addEventListener("click", () => {
        center.setAttribute("inert", "");
        center.setAttribute("aria-hidden", "true");
      });
  }
  focusContent();
  if (state === "go-to-line") openGoto().value = "50";
  if (state === "outline") {
    const symbol = openOutline();
    symbol.click();
    symbol.focus();
    Object.assign(context, {
      symbolLabel: symbol.getAttribute("aria-label"),
      symbolLine: 20,
      symbolColumn: 1,
    });
  }
  if (state === "search-view") openSearch().value = "export";
  if (state === "settings-editor") openSettings();
  return {
    dom,
    window,
    document,
    context,
    openGoto,
    observe: () => observeEditor(document, window, context, visibleRectangle),
  };
}
