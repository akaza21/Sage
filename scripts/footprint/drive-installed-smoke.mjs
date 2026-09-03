import { createHash } from "node:crypto";
import fs from "node:fs";

const [portValue, mode, rootLabel, outputPath] = process.argv.slice(2);
const port = Number(portValue);
const modes = new Set(["primary", "restart", "reinstall"]);
if (
  !Number.isInteger(port) ||
  port < 1024 ||
  port > 65535 ||
  !modes.has(mode) ||
  !rootLabel ||
  Buffer.byteLength(rootLabel, "utf8") > 128 ||
  !outputPath
) {
  throw new Error(
    "usage: drive-installed-smoke.mjs <port> <primary|restart|reinstall> <root-label> <output>",
  );
}

const limits = Object.freeze({
  discoveryMs: 20_000,
  actionMs: 15_000,
  maxTargets: 32,
  maxTargetBytes: 65_536,
  maxResultBytes: 32_768,
});
const assertions = {
  projectSeeded: false,
  fileOpened: false,
  fileEditedAndSaved: false,
  workspaceSearchMatched: false,
  gitStaged: false,
  gitCommitted: false,
  chatCreated: false,
  chatRenamed: false,
  chatSwitched: false,
  simulatorTurnCompleted: false,
  restartPersisted: false,
  reinstallVerified: false,
};

function bounded(value, maximum = 512) {
  return String(value ?? "").slice(0, maximum);
}

async function readTargets(response) {
  const reader = response.body.getReader();
  const chunks = [];
  let bytes = 0;
  while (true) {
    const { done, value } = await reader.read();
    if (done) break;
    bytes += value.byteLength;
    if (bytes > limits.maxTargetBytes) {
      await reader.cancel();
      throw new Error("installed-smoke-target-response-oversize");
    }
    chunks.push(value);
  }
  const targets = JSON.parse(Buffer.concat(chunks).toString("utf8"));
  if (!Array.isArray(targets) || targets.length > limits.maxTargets) {
    throw new Error("installed-smoke-target-count-invalid");
  }
  return targets;
}

async function discoverPage() {
  const deadline = Date.now() + limits.discoveryMs;
  while (Date.now() < deadline) {
    try {
      const response = await fetch(`http://127.0.0.1:${port}/json`);
      if (!response.ok) throw new Error("target-http-failed");
      const pages = (await readTargets(response)).filter(
        (target) =>
          target.type === "page" &&
          target.url === "http://tauri.localhost/" &&
          target.title === "Sage",
      );
      if (pages.length === 1) return pages[0];
    } catch {
      // Retry only until the fixed discovery deadline.
    }
    await new Promise((resolve) => setTimeout(resolve, 100));
  }
  throw new Error("installed-smoke-target-unavailable");
}

async function connect(page) {
  const socket = new WebSocket(page.webSocketDebuggerUrl);
  await Promise.race([
    new Promise((resolve, reject) => {
      socket.addEventListener("open", resolve, { once: true });
      socket.addEventListener("error", reject, { once: true });
    }),
    new Promise((_, reject) =>
      setTimeout(
        () => reject(new Error("installed-smoke-cdp-open-timeout")),
        10_000,
      ),
    ),
  ]);
  let nextId = 0;
  const pending = new Map();
  socket.addEventListener("message", (event) => {
    const message = JSON.parse(event.data);
    const waiter = pending.get(message.id);
    if (!waiter) return;
    pending.delete(message.id);
    if (message.error)
      waiter.reject(new Error("installed-smoke-cdp-command-failed"));
    else waiter.resolve(message.result);
  });
  async function call(method, params = {}) {
    const id = ++nextId;
    const response = new Promise((resolve, reject) =>
      pending.set(id, { resolve, reject }),
    );
    socket.send(JSON.stringify({ id, method, params }));
    return Promise.race([
      response,
      new Promise((_, reject) =>
        setTimeout(
          () => reject(new Error("installed-smoke-cdp-command-timeout")),
          10_000,
        ),
      ),
    ]);
  }
  async function evaluate(expression) {
    const result = await call("Runtime.evaluate", {
      expression,
      awaitPromise: true,
      returnByValue: true,
    });
    if (result.exceptionDetails)
      throw new Error("installed-smoke-evaluation-failed");
    return result.result.value;
  }
  async function waitFor(expression) {
    const deadline = Date.now() + limits.actionMs;
    while (Date.now() < deadline) {
      if (await evaluate(expression)) return;
      await new Promise((resolve) => setTimeout(resolve, 100));
    }
    throw new Error("installed-smoke-semantic-state-timeout");
  }
  return { socket, call, evaluate, waitFor };
}

function setValueExpression(selector, value, kind = "input") {
  return `(()=>{const e=document.querySelector(${JSON.stringify(selector)});if(!e)return false;const p=${kind === "textarea" ? "HTMLTextAreaElement" : "HTMLInputElement"}.prototype;Object.getOwnPropertyDescriptor(p,'value').set.call(e,${JSON.stringify(value)});e.dispatchEvent(new Event('input',{bubbles:true}));return true})()`;
}

async function openFixtureFile(client) {
  await client.evaluate(
    `(()=>{const e=document.querySelector('button[aria-label="Explorer"]');if(!e)return false;e.click();return true})()`,
  );
  await client.waitFor(
    `document.querySelectorAll('.tree[aria-label="Project files"]').length===1`,
  );
  const opened = await client.evaluate(
    `(()=>{const e=document.querySelector('button[aria-label="Open file README.md"]');if(!e)return false;e.click();return true})()`,
  );
  if (!opened) throw new Error("installed-smoke-fixture-file-missing");
  await client.waitFor(
    `document.querySelectorAll('[aria-label="Edit README.md"]').length===1`,
  );
  assertions.projectSeeded = true;
  assertions.fileOpened = true;
}

async function editSaveSearchAndCommit(client) {
  await client.evaluate(
    `document.querySelector('[aria-label="Edit README.md"]').focus()`,
  );
  await client.call("Input.dispatchKeyEvent", {
    type: "keyDown",
    key: "a",
    code: "KeyA",
    modifiers: 2,
  });
  await client.call("Input.dispatchKeyEvent", {
    type: "keyUp",
    key: "a",
    code: "KeyA",
    modifiers: 2,
  });
  await client.call("Input.insertText", {
    text: "sage installed smoke committed\n",
  });
  await client.waitFor(
    `[...document.querySelectorAll('.file-surface button')].some(e=>e.textContent.trim()==='Save'&&!e.disabled)`,
  );
  await client.evaluate(
    `(()=>{const e=[...document.querySelectorAll('.file-surface button')].find(e=>e.textContent.trim()==='Save'&&!e.disabled);e.click();return true})()`,
  );
  await client.waitFor(
    `[...document.querySelectorAll('.file-surface button')].some(e=>e.textContent.trim()==='Save'&&e.disabled)`,
  );
  assertions.fileEditedAndSaved = true;

  await client.evaluate(
    `document.querySelector('button[aria-label="Search"]').click()`,
  );
  await client.waitFor(
    `document.querySelectorAll('input[aria-label="Search text"]').length===1`,
  );
  await client.evaluate(
    setValueExpression('input[aria-label="Search text"]', "committed"),
  );
  await client.evaluate(
    `(()=>{const e=document.querySelector('.workspace-search__form');if(!e)return false;return e.dispatchEvent(new Event('submit',{bubbles:true,cancelable:true}))===false})()`,
  );
  await client.waitFor(
    `document.querySelectorAll('button[aria-label^="Open README.md at line "]').length===1`,
  );
  assertions.workspaceSearchMatched = true;

  await client.evaluate(
    `document.querySelector('button[aria-label="Source Control"]').click()`,
  );
  const stageLabel = `Stage README.md in ${rootLabel}`;
  await client.waitFor(
    `document.querySelectorAll(${JSON.stringify(`button[aria-label="${stageLabel}"]`)}).length===1`,
  );
  await client.evaluate(
    `document.querySelector(${JSON.stringify(`button[aria-label="${stageLabel}"]`)}).click()`,
  );
  await client.waitFor(
    `document.querySelectorAll('button[aria-label^="Unstage README.md in "]').length===1`,
  );
  assertions.gitStaged = true;
  await client.evaluate(
    setValueExpression(
      'textarea[placeholder="Commit message"]',
      "beta installed smoke",
      "textarea",
    ),
  );
  const commitLabel = `Commit staged changes in ${rootLabel}`;
  await client.waitFor(
    `document.querySelectorAll(${JSON.stringify(`button[aria-label="${commitLabel}"]:not([disabled])`)}).length===1`,
  );
  await client.evaluate(
    `document.querySelector(${JSON.stringify(`button[aria-label="${commitLabel}"]:not([disabled])`)}).click()`,
  );
  await client.waitFor(
    `document.querySelector('textarea[placeholder="Commit message"]')?.value===''`,
  );
  assertions.gitCommitted = true;
}

async function exerciseChats(client) {
  const before = await client.evaluate(
    `document.querySelectorAll('.project-chat:not(.project-chat--utility)').length`,
  );
  await client.evaluate(
    `document.querySelector('button[aria-label="New chat"]').click()`,
  );
  await client.waitFor(
    `document.querySelectorAll('.project-chat:not(.project-chat--utility)').length===${before + 1}`,
  );
  assertions.chatCreated = true;
  const renamed = await client.evaluate(
    `(()=>{const e=[...document.querySelectorAll('button[aria-label^="Rename chat "]')].at(-1);if(!e)return false;e.click();return true})()`,
  );
  if (!renamed) throw new Error("installed-smoke-chat-rename-missing");
  await client.waitFor(
    `document.querySelectorAll('input[aria-label^="New name for "]').length===1`,
  );
  await client.evaluate(
    setValueExpression(
      'input[aria-label^="New name for "]',
      "Installed smoke chat",
    ),
  );
  await client.evaluate(
    `document.querySelector('button[aria-label="Save chat name"]').click()`,
  );
  await client.waitFor(
    `[...document.querySelectorAll('.project-chat:not(.project-chat--utility)')].some(e=>e.textContent.includes('Installed smoke chat'))`,
  );
  assertions.chatRenamed = true;
  const switched = await client.evaluate(
    `(()=>{const e=[...document.querySelectorAll('.project-chat:not(.project-chat--utility)')];if(e.length<2)return false;e[0].click();return true})()`,
  );
  if (!switched) throw new Error("installed-smoke-chat-switch-failed");
  await client.waitFor(
    `document.querySelector('.project-chat--active')===[...document.querySelectorAll('.project-chat:not(.project-chat--utility)')][0]`,
  );
  await client.evaluate(
    `(()=>{const e=[...document.querySelectorAll('.project-chat:not(.project-chat--utility)')].find(e=>e.textContent.includes('Installed smoke chat'));if(!e)return false;e.click();return true})()`,
  );
  await client.waitFor(
    `document.querySelector('.project-chat--active')?.textContent.includes('Installed smoke chat')`,
  );
  assertions.chatSwitched = true;
  await client.evaluate(
    `(()=>{const e=document.querySelector('.select-control select');const o=[...e.options].find(o=>o.textContent.includes('Simulator'));if(!o)return false;e.value=o.value;e.dispatchEvent(new Event('change',{bubbles:true}));return true})()`,
  );
  await client.waitFor(`!document.querySelector('textarea#prompt')?.disabled`);
  await client.evaluate(
    setValueExpression("textarea#prompt", "installed smoke prompt", "textarea"),
  );
  await client.evaluate(
    `document.querySelector('button[aria-label="Send message"]').click()`,
  );
  await client.waitFor(
    `[...document.querySelectorAll('.message--assistant')].some(e=>e.textContent.includes('Simulator acknowledged'))`,
  );
  assertions.simulatorTurnCompleted = true;
}

async function verifyPersistence(client, key) {
  await client.waitFor(
    `[...document.querySelectorAll('.project-chat:not(.project-chat--utility)')].some(e=>e.textContent.includes('Installed smoke chat'))`,
  );
  await client.evaluate(
    `(()=>{const e=[...document.querySelectorAll('.project-chat:not(.project-chat--utility)')].find(e=>e.textContent.includes('Installed smoke chat'));e.click();return true})()`,
  );
  await client.waitFor(
    `[...document.querySelectorAll('.message--assistant')].some(e=>e.textContent.includes('Simulator acknowledged'))`,
  );
  await openFixtureFile(client);
  await client.waitFor(
    `document.querySelector('.cm-content')?.textContent.includes('sage installed smoke committed')`,
  );
  assertions[key] = true;
}

let socket;
try {
  const page = await discoverPage();
  const client = await connect(page);
  socket = client.socket;
  if (mode === "primary") {
    await openFixtureFile(client);
    await editSaveSearchAndCommit(client);
    await exerciseChats(client);
  } else {
    await verifyPersistence(
      client,
      mode === "restart" ? "restartPersisted" : "reinstallVerified",
    );
  }
  const payload = { schemaVersion: 1, mode, assertions, limits };
  const bytes = Buffer.from(`${JSON.stringify(payload, null, 2)}\n`);
  if (bytes.byteLength > limits.maxResultBytes)
    throw new Error("installed-smoke-result-oversize");
  fs.writeFileSync(outputPath, bytes, { flag: "wx" });
  process.stdout.write(
    JSON.stringify({
      evidenceSha256: createHash("sha256").update(bytes).digest("hex"),
      assertionCount: Object.values(assertions).filter(Boolean).length,
    }),
  );
} catch (error) {
  process.stderr.write(`${bounded(error?.message ?? error, 1024)}\n`);
  process.exitCode = 1;
} finally {
  socket?.close();
}
