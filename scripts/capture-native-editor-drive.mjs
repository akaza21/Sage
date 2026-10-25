import { waitForState } from "./capture-native-chat-contract.mjs";
import { boundedEvaluate } from "./capture-native-chat-shared.mjs";
import {
  scopedClickExpression,
  fillExpression,
} from "./capture-native-chat-parity-drive.mjs";
import { admitEditor, EDITOR_FILE } from "./capture-native-editor-catalog.mjs";
import {
  EDITOR_MODE,
  CONTENT,
  GOTO_INPUT,
  editorObservation,
} from "./capture-native-editor-dom.mjs";

export async function editorChord(cdp, key, deadline) {
  for (const type of ["keyDown", "keyUp"])
    await cdp.key(
      { type, key, ctrl: true },
      Math.min(deadline, Date.now() + 5000),
    );
}
export async function driveEditor(
  cdp,
  options,
  deadline,
  seed,
  record = () => {},
) {
  admitEditor(options);
  const evaluate = boundedEvaluate(cdp, deadline);
  const until = (expression) =>
    waitForState(() => evaluate(expression), deadline);
  const click = (selector) =>
    until(scopedClickExpression(selector, "document"));
  const fill = (selector, text) =>
    until(fillExpression(selector, "document", text));
  const focus = (selector) =>
    until(
      `(()=>{const nodes=document.querySelectorAll(${JSON.stringify(selector)});if(nodes.length!==1)return false;nodes[0].scrollIntoView({block:'nearest'});nodes[0].focus();return document.activeElement===nodes[0]})()`,
    );
  const tool = async (label) => {
    const selector = '.activity-rail button[aria-label="' + label + '"]';
    if (
      !(await evaluate(
        `document.querySelector(${JSON.stringify(selector)})?.getAttribute('aria-expanded')==='true'`,
      ))
    )
      await click(selector);
  };
  await until(
    `(()=>{const input=document.querySelector(${JSON.stringify(EDITOR_MODE)});if(!input)return false;if(!input.checked)input.click();return input.checked})()`,
  );
  await tool("Explorer");
  for (const folder of ["src", "src/features", "src/features/outline"]) {
    const selector =
      '[role="treeitem"][aria-label="Toggle folder ' + folder + '"]';
    await until(`Boolean(document.querySelector(${JSON.stringify(selector)}))`);
    if (
      !(await evaluate(
        `document.querySelector(${JSON.stringify(selector)}).getAttribute('aria-expanded')==='true'`,
      ))
    )
      await click(selector);
  }
  await click('[role="treeitem"][aria-label="Open file ' + EDITOR_FILE + '"]');
  await until(
    `document.querySelectorAll(${JSON.stringify(CONTENT)}).length===1`,
  );
  if (options.width < 761) {
    await until(
      `(()=>{const panel=document.querySelector('[aria-label="Explorer project tools"]');if(!panel)return true;panel.dispatchEvent(new KeyboardEvent('keydown',{key:'Escape',bubbles:true}));return true})()`,
    );
    await until("!document.querySelector('.center-region[inert]')");
  }
  await focus(CONTENT);
  await editorChord(cdp, "g", deadline);
  await fill(GOTO_INPUT, String(seed.midLine));
  if (options.state !== "go-to-line") {
    await until(
      `(()=>{const input=document.querySelector(${JSON.stringify(GOTO_INPUT)});if(!input)return true;input.dispatchEvent(new KeyboardEvent('keydown',{key:'Enter',bubbles:true}));return true})()`,
    );
    await until(`!document.querySelector(${JSON.stringify(GOTO_INPUT)})`);
  }
  const context = {
    state: options.state,
    file: EDITOR_FILE,
    midLine: seed.midLine,
  };
  if (options.state === "outline") {
    await tool("Outline");
    await until("document.querySelectorAll('.outline-card').length>0");
    const target = await evaluate(
      `(()=>{const nodes=[...document.querySelectorAll('.outline-card')];if(nodes.length>500)throw Error('editor-dom-bound');const node=nodes[Math.floor(nodes.length/2)];const label=node?.getAttribute('aria-label');const match=/, line ([0-9]+), column ([0-9]+)$/.exec(label??'');return match?{symbolLabel:label,symbolLine:Number(match[1]),symbolColumn:Number(match[2])}:null})()`,
    );
    if (!target) throw new Error("editor-symbol-missing");
    Object.assign(context, target);
    const selector =
      ".outline-card[aria-label=" + JSON.stringify(target.symbolLabel) + "]";
    await click(selector);
    await until(
      `document.querySelector('[title="Caret position"]')?.textContent==='Ln ${target.symbolLine}, Col ${target.symbolColumn}'`,
    );
    await focus(selector);
  } else if (options.state === "search-view") {
    await tool("Search");
    await fill('input[aria-label="Search text"]', "export");
  } else if (options.state === "settings-editor") {
    await click('button[aria-label="Open settings"]');
    await until("Boolean(document.querySelector('.settings-surface'))");
    await until(
      `(()=>{const tabs=[...document.querySelectorAll('.settings-surface__nav [role="tab"]')];const tab=tabs.find(node=>node.textContent.trim()==='Editor');if(!tab)return false;if(tab.getAttribute('aria-selected')!=='true')tab.click();return true})()`,
    );
  }
  const expression = editorObservation(context);
  await waitForState(
    async () => (await evaluate(expression)).predicates.namedState === true,
    deadline,
  );
  record("editor-state-ready");
  return { context, expression };
}
