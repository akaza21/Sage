import {
  closeSync,
  existsSync,
  lstatSync,
  mkdirSync,
  openSync,
  readSync,
  writeFileSync,
} from "node:fs";
import path from "node:path";

import {
  attachCdp,
  decodeBoundedScreenshot,
  discoverTarget,
  operationDeadline,
} from "./source-functional-native-ide-journey.mjs";
import {
  boundedRead,
  discoveryReady,
  freshRead,
  harnessPickerReadiness,
  harnessReadiness,
  pollDelay,
  pollReadiness,
  pollTurnAcceptance,
  probeExpiry,
  submissionReadiness,
  turnIdsPresent,
  turnOutcome,
  turnSettled,
} from "./hr6-readiness.mjs";
import {
  captureFocusedChatExpression,
  focusedChatSurfacePreamble,
  focusedRememberedChatSurfacePreamble,
  rememberedChatSurfacePreamble,
} from "./hr6-chat-scope.mjs";
import {
  harnessSelectionOutcome,
  includeOpenCodeRequested,
} from "./hr6-harness-selection.mjs";
import {
  TERMINAL_LIMITS,
  boundedHeadProjection,
  boundedFirstProjection,
  boundedIdentityProjection,
  boundedMatchProjection,
  boundedNodeTextProjection,
  boundedScalarProjection,
  boundedScopeProjection,
  boundedTextProjection,
  boundedTurnsProjection,
  terminalExpression,
  toolCardsProjection,
  toolRollupExpression,
} from "./hr6-terminal-expression.mjs";
import { READ_PROBE, toolEvidenceOutcome } from "./hr6-tool-evidence.mjs";
import {
  TOOL_TERMINAL_GRACE_MS,
  cancelRunningToolExpression,
  cancelledToolOutcome,
  runningToolReadiness,
  rollupRescanNeeded,
  toolTerminalGrace,
} from "./hr6-cancel-evidence.mjs";
import {
  permissionIdentity,
  permissionOutcome,
  permissionReadiness,
  permissionStateExpression,
  resolvePermissionExpression,
} from "./hr6-permission-evidence.mjs";
import {
  diffSurfaceExpression,
  diffSurfaceOutcome,
  fileChangeOutcome,
  fileChangesExpression,
  openDiffExpression,
} from "./hr6-diff-evidence.mjs";
import {
  APPROVAL_ACCESS_MODE,
  accessModeExpandedExpression,
  accessModeSettled,
  accessModeStateExpression,
  chooseAccessModeExpression,
  openAccessModeExpression,
} from "./hr6-access-mode.mjs";

const values = Object.create(null);
const TOOL_RESULT_PROMPT =
  "Read the file alpha.txt in this project with a read-only tool, then reply with its exact contents.";
const TOOL_CANCEL_PROMPT =
  "Using only read-only tools, read alpha.txt and then README.md in this project, one file per tool call, and repeat that pair eight times in sequence. Do not write, create, or modify any files. Then reply with the word DONE.";
const PERMISSION_FILE = "hr6-permission.txt";
const PERMISSION_LINE = "hr6 permission granted";
const DECLINE_FILE = "hr6-declined.txt";
const PERMISSION_EXPECTED = { command: "Created file" };
const SCRATCH_BYTE_LIMIT = 4096;
const DECLINE_EXPECTED = { command: "Created file" };
const LONG_TOOL_RELEASE_FILE = "hr6-release.txt";
const LONG_TOOL_COMMAND = `$hr6Deadline=(Get-Date).AddSeconds(120); while (-not (Test-Path -LiteralPath ${LONG_TOOL_RELEASE_FILE}) -and (Get-Date) -lt $hr6Deadline) { Start-Sleep -Seconds 2 }`;
const LONG_TOOL_EXPECTED = { command: LONG_TOOL_COMMAND };
const SCRATCH_ROOT_LABEL = "scratch";
const SCRATCH_UP = String.fromCharCode(46, 46);
const SCRATCH_TRAILING_NEWLINE = /\r?\n$/u;
const PERMISSION_PENDING_KINDS = ["permission-actionable"];
const DIFF_PENDING_KINDS = [
  "pane-absent",
  "diff-absent",
  "diff-counts-pending",
];
const PERMISSION_WRITE_PROMPT = `Create a new file named ${PERMISSION_FILE} in this project containing exactly the single line ${PERMISSION_LINE}. Use your file-writing tool. Do not read or modify any other file. Then reply with the word WROTE.`;
const PERMISSION_DECLINE_PROMPT = `Create a new file named ${DECLINE_FILE} in this project containing exactly the single line hr6 permission declined. Use your file-writing tool. Do not read or modify any other file.`;
const LONG_TOOL_PROMPT = `Use exactly one shell tool call in this project and no other tools, and run exactly this command in the foreground with nothing added, removed, or reordered: ${LONG_TOOL_COMMAND}
The command waits until ${LONG_TOOL_RELEASE_FILE} exists or its 120-second deadline passes. Do not run it in the background, do not substitute another wait, and do not write, create, or modify any files. Reply with the word SLEPT once the command finishes.`;
const COMPOSER_CONTEXT_PROJECTION = `(surface)=>{
  const LIMIT=96;
  const NODES=4;
  const BUDGET=2048;
  const find=(root,selector,limit)=>{
    if(!root||!root.children)return {nodes:[],exceeded:false};
    const found=[];
    const stack=[root];
    let visited=0;
    let exceeded=false;
    while(stack.length>0){
      const node=stack.pop();
      visited+=1;
      if(visited>BUDGET){exceeded=true;break;}
      if(node!==root&&node.matches&&node.matches(selector)){
        found.push(node);
        if(found.length>limit){exceeded=true;found.pop();break;}
        continue;
      }
      const children=node.children;
      const count=children?children.length:0;
      if(count>BUDGET){exceeded=true;break;}
      for(let index=count-1;index>=0;index-=1)stack.push(children[index]);
    }
    return {nodes:found,exceeded};
  };
  const walk=(root)=>{
    if(!root)return {text:"",truncated:false};
    const parts=[];
    const stack=[root];
    let visited=0;
    let total=0;
    let truncated=false;
    while(stack.length>0){
      const node=stack.pop();
      visited+=1;
      if(visited>BUDGET){truncated=true;break;}
      if(node.nodeType===3){
        const raw=node.nodeValue||"";
        const capped=raw.length>LIMIT*4?raw.slice(0,LIMIT*4):raw;
        const value=capped.trim();
        if(value.length>0){parts.push(value);total+=value.length+1;}
        if(raw.length>LIMIT*4||total>LIMIT*4){truncated=true;break;}
        continue;
      }
      const children=node.childNodes;
      const count=children?children.length:0;
      if(count>BUDGET){truncated=true;break;}
      for(let index=count-1;index>=0;index-=1)stack.push(children[index]);
    }
    return {text:parts.join(" "),truncated};
  };
  let truncated=false;
  const bound=(value)=>{
    if(typeof value!=="string")return null;
    const capped=value.length>LIMIT*4?value.slice(0,LIMIT*4):value;
    const text=capped.trim();
    if(capped!==value)truncated=true;
    if(text.length<=LIMIT)return text;
    truncated=true;
    return text.slice(0,LIMIT);
  };
  const read=(node)=>{
    const walked=walk(node);
    if(walked.truncated)truncated=true;
    return bound(walked.text);
  };
  const regions=find(surface,'p.composer__context[aria-label="Composer context"]',NODES);
  if(regions.nodes.length!==1)return {regionCount:regions.nodes.length,regionsExceeded:regions.exceeded,scanExceeded:false,rendered:false,truncated:false,labelCount:0,label:null,summary:null,meterCount:0,valueNow:null,valueMax:null,valueText:null};
  const region=regions.nodes[0];
  const labels=find(region,'.composer__context-usage-label',NODES);
  const meters=find(region,'.composer__context-usage[role="progressbar"]',NODES);
  const meter=meters.nodes.length===1?meters.nodes[0]:null;
  const summary=read(region);
  const label=labels.nodes.length===1?read(labels.nodes[0]):null;
  const valueNow=meter?bound(meter.getAttribute('aria-valuenow')):null;
  const valueMax=meter?bound(meter.getAttribute('aria-valuemax')):null;
  const valueText=meter?bound(meter.getAttribute('aria-valuetext')):null;
  const rendered=typeof summary==="string"&&summary.length>0;
  return {regionCount:1,regionsExceeded:regions.exceeded,scanExceeded:labels.exceeded||meters.exceeded,rendered,truncated,labelCount:labels.nodes.length,label,summary:rendered?summary:null,meterCount:meters.nodes.length,valueNow,valueMax,valueText};
}`;
for (let index = 2; index < process.argv.length; index += 2) {
  const key = process.argv[index];
  const value = process.argv[index + 1];
  if (!key?.startsWith("--") || value === undefined || values[key])
    throw new Error("hr6-arguments-invalid");
  values[key] = value;
}

const port = Number(values["--port"]);
const output = values["--output"];
const deadlineMs = Number(values["--deadline-ms"] ?? "150000");
if (
  !Number.isInteger(port) ||
  port < 1024 ||
  port > 65535 ||
  !output ||
  !path.isAbsolute(output) ||
  !Number.isInteger(deadlineMs) ||
  deadlineMs < 30000 ||
  deadlineMs > 600000
)
  throw new Error("hr6-arguments-invalid");

const includeOpenCode = includeOpenCodeRequested(values);

mkdirSync(output, { recursive: true });
const runnerStartedAt = Date.now();
const deadline = Date.now() + deadlineMs;
const observations = {
  schemaVersion: 1,
  port,
  startedAt: new Date().toISOString(),
  steps: [],
};

function pollYield(until = deadline) {
  const wait = pollDelay(Math.min(deadline, until), Date.now());
  return new Promise((resolve) => {
    setTimeout(resolve, wait);
  });
}

function expireAt(until) {
  return () =>
    new Promise((resolve) => {
      setTimeout(resolve, Math.max(0, Math.min(deadline, until) - Date.now()));
    });
}

function expression(value) {
  return `(()=>{${value}})()`;
}

function toolTerminalDeadline(
  actionDeadline,
  hardDeadline,
  graceStartedAt,
  graceMs,
) {
  if (!Number.isFinite(graceStartedAt)) return actionDeadline;
  return Math.min(
    hardDeadline,
    Math.max(actionDeadline, graceStartedAt + graceMs),
  );
}

async function evaluate(cdp, value, until = deadline) {
  return cdp.evaluate(value, operationDeadline(Math.min(deadline, until)));
}

async function probeRead(cdp, value, until) {
  if (Date.now() >= until) return { kind: "expired" };
  return boundedRead(() => evaluate(cdp, value, until), {
    expire: expireAt(probeExpiry(until, Date.now())),
  });
}

async function probeState(cdp, value, until, label, stage) {
  const read = await probeRead(cdp, value, until);
  if (read.kind === "error") throw read.error;
  if (read.kind === "value") return read.value;
  throw new Error(`hr6-unresponsive-${label}-${stage}`);
}

async function expandToolRollups(cdp, chat, turnId = null, until = deadline) {
  const opened = await evaluate(
    cdp,
    toolRollupExpression(
      focusedRememberedChatSurfacePreamble(chat, "return 0;"),
      turnId,
    ),
    until,
  );
  if (!opened) return opened;
  await pollYield(until);
  return opened;
}

async function waitFor(cdp, value, label, until = deadline) {
  while (Date.now() < until) {
    if (await evaluate(cdp, value, until)) return;
    await pollYield(until);
  }
  throw new Error(`hr6-timeout-${label}`);
}

async function shot(cdp, name, until = deadline) {
  const image = await cdp.screenshot(
    operationDeadline(Math.min(deadline, until)),
  );
  writeFileSync(
    path.join(output, `${name}.png`),
    decodeBoundedScreenshot(image),
    { flag: "wx" },
  );
}

function writeJson(name, value) {
  writeFileSync(
    path.join(output, `${name}.json`),
    `${JSON.stringify(value, null, 2)}\n`,
    { flag: "wx" },
  );
}

async function snapshot(cdp, chat, name, turnId = null) {
  await expandToolRollups(cdp, chat, turnId);
  const value = await evaluate(
    cdp,
    expression(`
      ${focusedRememberedChatSurfacePreamble(chat, "return null;")}
      const one=${boundedFirstProjection()};
      const scopeOf=${boundedScopeProjection()};
      const turnsOf=${boundedTurnsProjection()};
      const findAlerts=${boundedMatchProjection(TERMINAL_LIMITS.alerts)};
      const findOptions=${boundedMatchProjection(TERMINAL_LIMITS.options)};
      const findControls=${boundedMatchProjection(2)};
      const picker=one(surface,'.searchable-picker__trigger[aria-label="Harness"]');
      const list=picker?.getAttribute('aria-controls')?document.getElementById(picker.getAttribute('aria-controls')):null;
      const tools=${toolCardsProjection()};
      const bound=${boundedTextProjection()};
      const walk=${boundedNodeTextProjection()};
      const walkWide=${boundedNodeTextProjection(4096)};
      const head=${boundedHeadProjection()};
      const scalar=${boundedScalarProjection()};
      const identity=${boundedIdentityProjection()};
      const composerScope=scopeOf(surface,'.composer');
      const cancels=findControls(composerScope,'button[aria-label="Cancel turn"]',false);
      const alerts=findAlerts(surface,'[role="alert"]',false);
      const options=findOptions(list,'[role="option"]',false);
      const scan=turnsOf(surface);
      const target=${JSON.stringify(turnId)};
      const scoped=target===null?scan.turns:scan.turns.filter(x=>x.dataset.turnId===target);
      const toolRoot=target===null?surface:scoped.length===1?scoped[0]:null;
      const toolScan=toolRoot?tools(toolRoot):{cards:[],exceeded:false};
      return {
        title:scalar(document.title),
        surfaceText:walkWide(surface).text,
        harnessTrigger:walk(picker).text||null,
        harnessExpanded:scalar(picker?.getAttribute('aria-expanded'))||null,
        harnessOptions:head(options.nodes,${TERMINAL_LIMITS.options},x=>({text:walk(x).text,disabled:x.getAttribute('aria-disabled')==='true'||x.disabled})),
        harnessOptionsExceeded:options.exceeded,
        sessionStatus:walk(one(surface,'.session-state')).text||null,
        alerts:head(alerts.nodes,${TERMINAL_LIMITS.alerts},x=>bound(walk(x).text).text),
        alertsExceeded:alerts.exceeded,
        composerDisabled:one(composerScope,'textarea[aria-label="Message agent"]')?.disabled??null,
        cancelVisible:cancels.nodes.length,
        toolCards:toolScan.cards.length,
        toolStates:toolScan.cards,
        toolsExceeded:toolScan.exceeded,
        turnScope:identity(target),
        turnScopeResolved:target===null?null:scoped.length===1,
        composerContext:(${COMPOSER_CONTEXT_PROJECTION})(surface),
        turnCount:scan.exceeded?null:scan.turns.length,
        turnsExceeded:scan.exceeded,
        turns:head(scoped,${TERMINAL_LIMITS.turns},x=>{const body=walkWide(x);return{id:identity(x.dataset.turnId),status:scalar(one(x,'[data-turn-status]')?.getAttribute('aria-label'))||null,text:body.text,textTruncated:body.truncated};})
      };
    `),
  );
  observations.steps.push({ name, value });
  writeJson(name, value);
  await shot(cdp, name);
  return value;
}

async function click(cdp, value, label) {
  const clicked = await evaluate(cdp, expression(value));
  if (clicked !== true) throw new Error(`hr6-action-${label}`);
}

function reportedUsageOutcome(value) {
  if (!value || value.regionsExceeded !== false) return "region";
  if (value.regionCount === 0) return "unmetered";
  if (value.regionCount !== 1) return "region";
  if (value.scanExceeded !== false) return "scan";
  if (value.truncated !== false) return "oversized";
  if (value.rendered !== true) return "absent";
  if (value.labelCount === 0 && value.meterCount === 0) return "shape";
  if (value.labelCount !== 1 || value.meterCount !== 1) return "shape";
  const now = usageAttributeInteger(value.valueNow);
  const max = usageAttributeInteger(value.valueMax);
  const meter = String(value.valueText ?? "").match(
    /^(\d[\d,]*) \/ (\d[\d,]*) tokens used$/u,
  );
  if (now === null || max === null || max <= 0 || !meter) return "meter";
  const total = usageGroupedInteger(meter[1]);
  const capacity = usageGroupedInteger(meter[2]);
  if (total === null || capacity === null || capacity !== max) return "meter";
  if (now !== Math.min(Math.max(total, 0), max)) return "meter";
  const displayed = String(value.label ?? "").match(
    /^(\d[\d,]*)(K?) context$/u,
  );
  const count = displayed ? usageGroupedInteger(displayed[1]) : null;
  if (count === null) return "label";
  const compact = total >= 1000 && total % 1000 === 0;
  if (displayed[2] === "K")
    return compact && count * 1000 === total ? "usage" : "label";
  return !compact && count === total ? "usage" : "label";
}

function usageAttributeInteger(value) {
  if (typeof value !== "string" || !/^\d+$/u.test(value)) return null;
  const parsed = Number(value);
  return Number.isSafeInteger(parsed) ? parsed : null;
}

function usageGroupedInteger(value) {
  const parsed = Number(value.replace(/,/gu, ""));
  return Number.isSafeInteger(parsed) && parsed >= 0 ? parsed : null;
}

async function assertReportedUsage(cdp, chat, label) {
  const usageDeadline = Math.min(deadline, Date.now() + 15_000);
  let value = null;
  let outcome = "region";
  while (Date.now() < usageDeadline) {
    const read = await probeRead(
      cdp,
      expression(`
        ${focusedRememberedChatSurfacePreamble(chat, "return null;")}
        return (${COMPOSER_CONTEXT_PROJECTION})(surface);
      `),
      usageDeadline,
    );
    if (read.kind === "error") throw read.error;
    if (read.kind === "expired") break;
    if (read.kind === "unresponsive") {
      observations.steps.push({
        name: `${label}-reported-usage-unresponsive`,
        value: { outcome, composerContext: value },
      });
      throw new Error(`hr6-unresponsive-${label}-reported-usage`);
    }
    value = read.value;
    outcome = reportedUsageOutcome(value);
    if (outcome === "usage") break;
    await pollYield(usageDeadline);
  }
  observations.steps.push({
    name: `${label}-reported-usage`,
    value: { outcome, composerContext: value },
  });
  if (outcome === "usage") return;
  await snapshot(cdp, chat, `${label}-reported-usage-${outcome}`);
  throw new Error(`hr6-reported-usage-${outcome}-${label}`);
}

function markClaudePhase(label, phase) {
  if (label !== "07-claude-pong") return;
  observations.steps.push({
    name: `${label}-${phase}`,
    value: { runnerElapsedMs: Date.now() - runnerStartedAt },
  });
}

async function turnAcceptanceSample(cdp, chat, until = deadline) {
  const left = chat === "claude" ? "codex" : "claude";
  return evaluate(
    cdp,
    expression(`
      const chats=window.__hr6Chats??Object.create(null);
      const find=${boundedMatchProjection(2)};
      const one=${boundedFirstProjection()};
      const scopeOf=${boundedScopeProjection()};
      const turnsOf=${boundedTurnsProjection()};
      const walk=${boundedNodeTextProjection(160)};
      const state=(name)=>{
        const pane=chats[name];
        if(!pane||!pane.isConnected)return null;
        const surfaces=find(pane,'section.chat-surface[aria-label="Agent chat"]',false,"section.acp-turn");
        if(surfaces.nodes.length!==1||surfaces.exceeded)return null;
        const surface=surfaces.nodes[0];
        const composerScope=scopeOf(surface,".composer");
        const input=one(composerScope,'textarea[aria-label="Message agent"]');
        const scan=turnsOf(surface);
        const identity=${boundedIdentityProjection()};
        const newest=identity(scan.turns.at(-1)?.dataset.turnId);
        const status=walk(one(surface,".session-state")).text;
        const statusClass=!status?"none":status==="Ready"?"ready":/sign[ -]?in|consent/iu.test(status)?"auth":/unavailable/iu.test(status)?"unavailable":"transitioning";
        const alerts=find(surface,'[role="alert"]',false,"section.acp-turn");
        return {present:true,turnCount:scan.exceeded?null:scan.turns.length,turnsExceeded:scan.exceeded,newestTurnId:newest,composerHasValue:Boolean(input?.value),composerDisabled:input?.disabled??null,statusClass,alertClass:alerts.exceeded!==false||alerts.nodes.length>0?"present":"none"};
      };
      const focused=find(document.documentElement,"section.agent-chat-pane.agent-chat-pane--focused",false,"section.acp-turn");
      const matches=focused.nodes.length===1&&!focused.exceeded?[${JSON.stringify(left)},${JSON.stringify(chat)}].filter(name=>chats[name]===focused.nodes[0]):[];
      return {focusedPane:matches.length===1?matches[0]:focused.nodes.length===0?null:"ambiguous",left:state(${JSON.stringify(left)}),right:state(${JSON.stringify(chat)})};
    `),
    until,
  );
}

async function waitForAcceptedTurn(cdp, chat, initial, label) {
  const acceptanceDeadline = Math.min(deadline, Date.now() + 25_000);
  const result = await pollTurnAcceptance(
    () => turnAcceptanceSample(cdp, chat, acceptanceDeadline),
    initial,
    {
      now: Date.now,
      until: acceptanceDeadline,
      wait: () => new Promise((resolve) => setTimeout(resolve, 250)),
    },
  );
  const value = {
    runnerElapsedMs: Date.now() - runnerStartedAt,
    outcome: result.outcome,
    samples: result.samples,
  };
  observations.steps.push({
    name: `${label}-turn-acceptance`,
    value,
  });
  if (result.outcome === "accepted") return;
  writeJson(`${label}-turn-acceptance`, value);
  await shot(
    cdp,
    `${label}-turn-acceptance`,
    Math.min(deadline, Date.now() + 5_000),
  ).catch(() => {});
  throw new Error(`hr6-turn-acceptance-${result.outcome}-${label}`);
}

async function rememberFocusedChat(cdp, name, label) {
  const captured = await evaluate(
    cdp,
    expression(captureFocusedChatExpression(name, "return false;")),
  );
  if (captured !== true) throw new Error(`hr6-chat-capture-${label}`);
}

async function focusRememberedChat(cdp, name, label) {
  await click(
    cdp,
    `${rememberedChatSurfacePreamble(name, "return false;")}const find=${boundedMatchProjection(2)};const controls=find(pane,'button.agent-chat-pane__focus',false,"section.acp-turn");if(controls.nodes.length!==1||controls.exceeded)return false;controls.nodes[0].click();return true;`,
    `${label}-focus`,
  );
  await waitFor(
    cdp,
    expression(
      `${focusedRememberedChatSurfacePreamble(name, "return false;")}return true;`,
    ),
    `${label}-focused`,
  );
}

async function assertRememberedHarness(cdp, name, expected, label) {
  const actual = await evaluate(
    cdp,
    expression(`
      ${rememberedChatSurfacePreamble(name, "return null;")}
      const find=${boundedMatchProjection(2)};
      const walk=${boundedNodeTextProjection(160)};
      const triggers=find(surface,'.searchable-picker__trigger[aria-label="Harness"]',false,"section.acp-turn");
      if(triggers.nodes.length!==1||triggers.exceeded)return null;
      return walk(triggers.nodes[0]).text||null;
    `),
  );
  observations.steps.push({ name: `${label}-harness`, value: actual });
  if (typeof actual !== "string" || !actual.includes(expected))
    throw new Error(`hr6-harness-mismatch-${label}`);
}

async function readRememberedTurnState(cdp, name, until = deadline) {
  const read = await probeRead(
    cdp,
    expression(`
      ${focusedRememberedChatSurfacePreamble(name, "return null;")}
      const one=${boundedFirstProjection()};
      const scopeOf=${boundedScopeProjection()};
      const turnsOf=${boundedTurnsProjection()};
      const findAlerts=${boundedMatchProjection(TERMINAL_LIMITS.alerts)};
      const walk=${boundedNodeTextProjection()};
      const head=${boundedHeadProjection()};
      const input=one(scopeOf(surface,".composer"),'textarea[aria-label="Message agent"]');
      const scan=turnsOf(surface);
      const identity=${boundedIdentityProjection()};
     const turnIds=head(scan.turns,${TERMINAL_LIMITS.turns},x=>identity(x.dataset.turnId)).filter(x=>x!==null);
      const alerts=findAlerts(surface,'[role="alert"]',false,"section.acp-turn");
      return {turnIds,turnsExceeded:scan.exceeded,alerts:head(alerts.nodes,${TERMINAL_LIMITS.alerts},x=>walk(x).text),alertsExceeded:alerts.exceeded,disabled:input?.disabled??null,status:walk(one(surface,'.session-state')).text||null};
    `),
    until,
  );
  if (read.kind === "error") throw read.error;
  return read.kind === "value" ? read.value : null;
}

async function rememberTurnIds(cdp, name, label) {
  const state = await readRememberedTurnState(cdp, name);
  if (!state || !Array.isArray(state.turnIds) || state.turnIds.length === 0)
    throw new Error(`hr6-turn-continuity-capture-${label}`);
  observations.steps.push({ name: `${label}-turn-ids`, value: state.turnIds });
  return state.turnIds;
}

async function assertRememberedTurnContinuity(
  cdp,
  name,
  expectedTurnIds,
  label,
) {
  const continuityDeadline = Math.min(deadline, Date.now() + 5000);
  const readState = async () => {
    const fresh = await freshRead(
      () => readRememberedTurnState(cdp, name, continuityDeadline),
      { now: () => Date.now(), until: continuityDeadline },
    );
    return fresh.kind === "value" ? fresh.value : null;
  };
  let state = await readState();
  if (state && submissionReadiness(state) === "transitioning") {
    while (Date.now() < continuityDeadline) {
      if (turnIdsPresent(expectedTurnIds, state.turnIds)) break;
      await pollYield(continuityDeadline);
      state = await readState();
      if (state === null) break;
    }
  }
  observations.steps.push({
    name: `${label}-transcript-continuity`,
    value: { expectedTurnIds, state },
  });
  if (!state || !turnIdsPresent(expectedTurnIds, state.turnIds)) {
    await snapshot(cdp, name, `${label}-transcript-loss`);
    throw new Error(`hr6-transcript-loss-${label}`);
  }
}

async function waitForHarnessPickerReadiness(cdp, chat, label) {
  const startedAt = Date.now();
  const readinessDeadline = Math.min(deadline, Date.now() + 45_000);
  const result = await pollReadiness(
    () =>
      probeState(
        cdp,
        expression(`
        ${focusedRememberedChatSurfacePreamble(chat, "return null;")}
        const one=${boundedFirstProjection()};
        const walkWide=${boundedNodeTextProjection(4096)};
        const findAlerts=${boundedMatchProjection(TERMINAL_LIMITS.alerts)};
       const trigger=one(surface,'.searchable-picker__trigger[aria-label="Harness"]');
        const alerts=findAlerts(surface,'[role="alert"]',false,"section.acp-turn");
        return {triggerPresent:Boolean(trigger),triggerDisabled:trigger?.disabled??null,projectLoading:/loading the authoritative project|wait for the project to finish loading/iu.test(walkWide(surface).text),alertCount:alerts.nodes.length,alertsExceeded:alerts.exceeded};
      `),
        readinessDeadline,
        label,
        "harness-picker",
      ),
    harnessPickerReadiness,
    {
      until: readinessDeadline,
      now: Date.now,
      yieldFrame: () => pollYield(readinessDeadline),
    },
  );
  if (result.status === "ready") {
    observations.steps.push({
      name: `${label}-project-authority`,
      value: {
        waitElapsedMs: Date.now() - startedAt,
        runnerElapsedMs: Date.now() - runnerStartedAt,
        state: result.state,
      },
    });
    return;
  }
  if (result.status === "scopeLost") {
    observations.steps.push({
      name: `${label}-project-authority-scope-lost`,
      value: {
        waitElapsedMs: Date.now() - startedAt,
        runnerElapsedMs: Date.now() - runnerStartedAt,
      },
    });
    throw new Error(`hr6-project-authority-scope-lost-${label}`);
  }
  if (result.status === "persistent" || result.status === "error") {
    observations.steps.push({
      name: `${label}-project-authority-${result.status}`,
      value: {
        waitElapsedMs: Date.now() - startedAt,
        runnerElapsedMs: Date.now() - runnerStartedAt,
        state: result.state,
      },
    });
    await snapshot(cdp, chat, `${label}-project-authority-${result.status}`);
    throw new Error(`hr6-project-authority-${result.status}-${label}`);
  }
  observations.steps.push({
    name: `${label}-project-authority-timeout`,
    value: {
      waitElapsedMs: Date.now() - startedAt,
      runnerElapsedMs: Date.now() - runnerStartedAt,
      state: result.state,
    },
  });
  await snapshot(cdp, chat, `${label}-project-authority-timeout`);
  throw new Error(`hr6-timeout-${label}-project-authority`);
}

async function selectHarness(cdp, chat, fragment, label, selection = null) {
  await waitForHarnessPickerReadiness(cdp, chat, label);
  await click(
    cdp,
    `${focusedRememberedChatSurfacePreamble(chat, "return false;")}const one=${boundedFirstProjection()};const x=one(surface,'.searchable-picker__trigger[aria-label="Harness"]');if(!x||x.disabled)return false;if(x.getAttribute('aria-expanded')==='false')x.click();return true;`,
    `${label}-open`,
  );
  try {
    await waitFor(
      cdp,
      expression(
        `${focusedRememberedChatSurfacePreamble(chat, "return false;")}const one=${boundedFirstProjection()};const x=one(surface,'.searchable-picker__trigger[aria-label="Harness"]');return x?.getAttribute('aria-expanded')==='true';`,
      ),
      `${label}-picker`,
      Math.min(deadline, Date.now() + 10_000),
    );
  } catch (error) {
    observations.steps.push({
      name: `${label}-picker-unavailable`,
      value: await evaluate(
        cdp,
        expression(`
          ${focusedRememberedChatSurfacePreamble(chat, "return null;")}
          const one=${boundedFirstProjection()};
          const walk=${boundedNodeTextProjection(160)};
          const x=one(surface,'.searchable-picker__trigger[aria-label="Harness"]');
          return x?{disabled:x.disabled,expanded:x.getAttribute('aria-expanded'),text:walk(x).text||null}:null;
        `),
      ),
    });
    await snapshot(cdp, chat, `${label}-picker-unavailable`);
    throw error;
  }
  await snapshot(cdp, chat, `${label}-picker`);
  let options = await evaluate(
    cdp,
    expression(`
      ${focusedRememberedChatSurfacePreamble(chat, "return [];")}
      const one=${boundedFirstProjection()};
      const find=${boundedMatchProjection(TERMINAL_LIMITS.options)};
      const walk=${boundedNodeTextProjection(160)};
      const head=${boundedHeadProjection()};
      const trigger=one(surface,'.searchable-picker__trigger[aria-label="Harness"]');
      const list=trigger?.getAttribute('aria-controls')?document.getElementById(trigger.getAttribute('aria-controls')):null;
      return head(find(list,'[role="option"]',false).nodes,${TERMINAL_LIMITS.options},x=>({text:walk(x).text}));
    `),
  );
  if (!discoveryReady(options)) {
    await waitFor(
      cdp,
      expression(`
        ${focusedRememberedChatSurfacePreamble(chat, "return false;")}
        const one=${boundedFirstProjection()};
        const find=${boundedMatchProjection(TERMINAL_LIMITS.options)};
        const walk=${boundedNodeTextProjection(160)};
        const trigger=one(surface,'.searchable-picker__trigger[aria-label="Harness"]');
        const list=trigger?.getAttribute('aria-controls')?document.getElementById(trigger.getAttribute('aria-controls')):null;
        const options=find(list,'[role="option"]',false).nodes;
        return options.length>0&&options.every(x=>!/(?:checking|detecting)/iu.test(walk(x).text));
      `),
      `${label}-discovery`,
    );
    options = await evaluate(
      cdp,
      expression(`
        ${focusedRememberedChatSurfacePreamble(chat, "return [];")}
        const one=${boundedFirstProjection()};
        const find=${boundedMatchProjection(TERMINAL_LIMITS.options)};
        const walk=${boundedNodeTextProjection(160)};
        const head=${boundedHeadProjection()};
        const trigger=one(surface,'.searchable-picker__trigger[aria-label="Harness"]');
        const list=trigger?.getAttribute('aria-controls')?document.getElementById(trigger.getAttribute('aria-controls')):null;
        return head(find(list,'[role="option"]',false).nodes,${TERMINAL_LIMITS.options},x=>({text:walk(x).text}));
      `),
    );
  }
  observations.steps.push({ name: `${label}-discovery`, value: options });
  const result = await evaluate(
    cdp,
    expression(`
    ${focusedRememberedChatSurfacePreamble(chat, "return 'absent';")}
    const one=${boundedFirstProjection()};
    const find=${boundedMatchProjection(TERMINAL_LIMITS.options)};
    const walk=${boundedNodeTextProjection(160)};
    const trigger=one(surface,'.searchable-picker__trigger[aria-label="Harness"]');
    const list=trigger?.getAttribute('aria-controls')?document.getElementById(trigger.getAttribute('aria-controls')):null;
    const option=find(list,'[role="option"]',false).nodes.find(x=>walk(x).text.includes(${JSON.stringify(fragment)}));
    if(!option)return 'absent';
    if(option.getAttribute('aria-disabled')==='true'||option.disabled)return 'disabled';
    option.click();return 'selected';
  `),
  );
  observations.steps.push({ name: `${label}-selection`, value: result });
  if (selection) selection.state = result;
  if (result !== "selected") return false;
  await waitFor(
    cdp,
    expression(`
      ${focusedRememberedChatSurfacePreamble(chat, "return false;")}
      const one=${boundedFirstProjection()};
      const walk=${boundedNodeTextProjection(160)};
      const x=one(surface,'.searchable-picker__trigger[aria-label="Harness"]');
      return x?.getAttribute('aria-expanded')==='false'&&walk(x).text.includes(${JSON.stringify(fragment)});
    `),
    `${label}-selected`,
  );
  const readyDeadline = Math.min(deadline, Date.now() + 30000);
  let readiness = null;
  while (Date.now() < readyDeadline) {
    const fresh = await freshRead(
      () =>
        probeState(
          cdp,
          expression(`
        ${focusedRememberedChatSurfacePreamble(chat, "return null;")}
        const one=${boundedFirstProjection()};
        const scopeOf=${boundedScopeProjection()};
        const findAlerts=${boundedMatchProjection(TERMINAL_LIMITS.alerts)};
        const walk=${boundedNodeTextProjection(160)};
        const head=${boundedHeadProjection()};
        const alerts=findAlerts(surface,'[role="alert"]',false,"section.acp-turn");
        return {status:walk(one(surface,'.session-state')).text||null,composerDisabled:one(scopeOf(surface,".composer"),'textarea[aria-label="Message agent"]')?.disabled??null,alerts:head(alerts.nodes,${TERMINAL_LIMITS.alerts},x=>walk(x).text),alertsExceeded:alerts.exceeded};
          `),
          readyDeadline,
          label,
          "harness-readiness",
        ),
      { now: () => Date.now(), until: readyDeadline },
    );
    if (fresh.kind === "late") {
      readiness = null;
      break;
    }
    readiness = fresh.value;
    if (harnessReadiness(readiness)) break;
    await pollYield(readyDeadline);
  }
  if (!harnessReadiness(readiness)) {
    observations.steps.push({
      name: `${label}-readiness-timeout`,
      value: readiness,
    });
    if (selection) selection.state = "readiness-timeout";
    await snapshot(cdp, chat, `${label}-selected`);
    return false;
  }
  if (readiness.alerts.length > 0) {
    observations.steps.push({
      name: `${label}-readiness-error`,
      value: readiness,
    });
    await snapshot(cdp, chat, `${label}-selected`);
    throw new Error(`hr6-harness-alert-${label}`);
  }
  if (readiness.composerDisabled) {
    observations.steps.push({ name: `${label}-alternative`, value: readiness });
    if (selection) selection.state = "alternative";
    await snapshot(cdp, chat, `${label}-selected`);
    return false;
  }
  observations.steps.push({ name: `${label}-readiness`, value: readiness });
  await snapshot(cdp, chat, `${label}-selected`);
  return true;
}

function scratchPath(relativePath) {
  const root = path.join(output, "scratch");
  if (
    !existsSync(path.join(root, "README.md")) ||
    !existsSync(path.join(root, "alpha.txt"))
  )
    throw new Error("hr6-scratch-fixture-unavailable");
  if (
    typeof relativePath !== "string" ||
    relativePath.length === 0 ||
    relativePath.startsWith("/") ||
    relativePath.includes(String.fromCharCode(92)) ||
    relativePath
      .split("/")
      .some((part) => part.length === 0 || part === "." || part === SCRATCH_UP)
  )
    throw new Error("hr6-scratch-path-invalid");
  return path.join(root, relativePath);
}

function scratchStat(file) {
  try {
    return lstatSync(file);
  } catch {
    return null;
  }
}

function readScratchText(file, size) {
  const handle = openSync(file, "r");
  try {
    const buffer = Buffer.alloc(Math.min(size, SCRATCH_BYTE_LIMIT));
    const read = readSync(handle, buffer, 0, buffer.length, 0);
    return buffer.subarray(0, read).toString("utf8");
  } finally {
    closeSync(handle);
  }
}

function assertScratchFile(relativePath, expected, label) {
  const file = scratchPath(relativePath);
  const stats = scratchStat(file);
  if (stats === null) {
    observations.steps.push({
      name: `${label}-scratch-file-absent`,
      value: { relativePath },
    });
    throw new Error(`hr6-scratch-file-absent-${label}`);
  }
  if (stats.isSymbolicLink() || !stats.isFile()) {
    observations.steps.push({
      name: `${label}-scratch-file-irregular`,
      value: { relativePath, symlink: stats.isSymbolicLink() },
    });
    throw new Error(`hr6-scratch-file-irregular-${label}`);
  }
  if (stats.size > SCRATCH_BYTE_LIMIT) {
    observations.steps.push({
      name: `${label}-scratch-file-oversized`,
      value: { relativePath, bytes: stats.size, limit: SCRATCH_BYTE_LIMIT },
    });
    throw new Error(`hr6-scratch-file-oversized-${label}`);
  }
  const text = readScratchText(file, stats.size);
  const trimmed = text.replace(SCRATCH_TRAILING_NEWLINE, "");
  if (trimmed !== expected) {
    observations.steps.push({
      name: `${label}-scratch-file-mismatch`,
      value: { relativePath, bytes: text.length },
    });
    throw new Error(`hr6-scratch-file-mismatch-${label}`);
  }
  observations.steps.push({
    name: `${label}-scratch-file`,
    value: { relativePath, bytes: text.length },
  });
}

function assertScratchAbsent(relativePath, label) {
  const file = scratchPath(relativePath);
  const stats = scratchStat(file);
  if (stats !== null) {
    observations.steps.push({
      name: `${label}-scratch-file-present`,
      value: { relativePath, symlink: stats.isSymbolicLink() },
    });
    throw new Error(`hr6-scratch-file-present-${label}`);
  }
  observations.steps.push({
    name: `${label}-scratch-file-declined`,
    value: { relativePath },
  });
}

async function freshState(cdp, chat, value, until, label, stage, turnId) {
  const fresh = await freshRead(
    () => probeState(cdp, value, until, label, stage),
    { now: () => Date.now(), until },
  );
  if (fresh.kind === "value") return fresh.value;
  observations.steps.push({
    name: `${label}-${stage}-late`,
    value: { stage, kind: fresh.kind },
  });
  await snapshot(cdp, chat, `${label}-${stage}-late`, turnId);
  throw new Error(`hr6-late-${label}-${stage}`);
}

async function selectAccessMode(cdp, chat, id, label) {
  const modeDeadline = Math.min(deadline, Date.now() + 30_000);
  const stateExpression = accessModeStateExpression(
    focusedRememberedChatSurfacePreamble(chat, 'return {kind:"scope-lost"};'),
  );
  let state = await freshState(
    cdp,
    chat,
    stateExpression,
    modeDeadline,
    label,
    "access-mode-read",
  );
  let readiness = accessModeSettled(state, id, Date.now(), modeDeadline);
  observations.steps.push({ name: `${label}-before`, value: state });
  if (readiness !== null && readiness.kind !== "access-mode-mismatch") {
    observations.steps.push({
      name: `${label}-${readiness.kind}`,
      value: { state, readiness },
    });
    await snapshot(cdp, chat, `${label}-${readiness.kind}`);
    throw new Error(`hr6-${readiness.kind}-${label}`);
  }
  if (readiness === null) return state;
  const opened = await evaluate(
    cdp,
    openAccessModeExpression(
      focusedRememberedChatSurfacePreamble(chat, "return false;"),
    ),
    modeDeadline,
  );
  if (opened !== true) {
    observations.steps.push({ name: `${label}-unopened`, value: state });
    await snapshot(cdp, chat, `${label}-unopened`);
    throw new Error(`hr6-access-mode-unopened-${label}`);
  }
  await waitFor(
    cdp,
    accessModeExpandedExpression(
      focusedRememberedChatSurfacePreamble(chat, "return false;"),
    ),
    `${label}-picker`,
    Math.min(modeDeadline, Date.now() + 10_000),
  );
  await snapshot(cdp, chat, `${label}-picker`);
  const chosen = await evaluate(
    cdp,
    chooseAccessModeExpression(
      focusedRememberedChatSurfacePreamble(
        chat,
        'return {outcome:"scope-lost"};',
      ),
      id,
    ),
    modeDeadline,
  );
  observations.steps.push({ name: `${label}-selection`, value: chosen });
  if (chosen?.outcome !== "selected") {
    await snapshot(cdp, chat, `${label}-refused`);
    throw new Error(
      `hr6-access-mode-${chosen?.outcome ?? "unreadable"}-${label}`,
    );
  }
  while (Date.now() < modeDeadline) {
    state = await freshState(
      cdp,
      chat,
      stateExpression,
      modeDeadline,
      label,
      "access-mode-recheck",
    );
    readiness = accessModeSettled(state, id, Date.now(), modeDeadline);
    if (readiness === null) break;
    if (readiness.kind !== "access-mode-mismatch") {
      observations.steps.push({
        name: `${label}-${readiness.kind}`,
        value: { state, readiness },
      });
      await snapshot(cdp, chat, `${label}-${readiness.kind}`);
      throw new Error(`hr6-${readiness.kind}-${label}`);
    }
    await pollYield(modeDeadline);
  }
  if (readiness !== null) {
    observations.steps.push({
      name: `${label}-timeout`,
      value: { state, readiness },
    });
    await snapshot(cdp, chat, `${label}-timeout`);
    throw new Error(`hr6-timeout-${label}-access-mode`);
  }
  observations.steps.push({ name: `${label}-selected`, value: state });
  await snapshot(cdp, chat, `${label}-selected`);
  return state;
}

async function resolvePermission(cdp, chat, turnId, decision, expected, label) {
  const permissionDeadline = Math.min(deadline, Date.now() + 90_000);
  const stateExpression = permissionStateExpression(
    focusedRememberedChatSurfacePreamble(chat, 'return {kind:"scope-lost"};'),
    turnId,
    expected,
  );
  let readiness = null;
  let state = null;
  while (Date.now() < permissionDeadline) {
    state = await freshState(
      cdp,
      chat,
      stateExpression,
      permissionDeadline,
      label,
      "permission-read",
      turnId,
    );
    readiness = permissionReadiness(state);
    if (readiness?.kind === "permission-waiting") break;
    if (readiness) {
      observations.steps.push({
        name: `${label}-permission-${readiness.kind}`,
        value: { state, readiness },
      });
      await snapshot(
        cdp,
        chat,
        `${label}-permission-${readiness.kind}`,
        turnId,
      );
      throw new Error(`hr6-permission-${readiness.kind}-${label}`);
    }
    await pollYield(permissionDeadline);
  }
  if (readiness?.kind !== "permission-waiting") {
    observations.steps.push({
      name: `${label}-permission-waiting-timeout`,
      value: { state, readiness },
    });
    await snapshot(cdp, chat, `${label}-permission-waiting-timeout`, turnId);
    throw new Error(`hr6-timeout-${label}-permission-waiting`);
  }
  observations.steps.push({
    name: `${label}-permission-waiting`,
    value: { card: readiness.card, decision },
  });
  await snapshot(cdp, chat, `${label}-permission-waiting`, turnId);
  const identity = { ...permissionIdentity(readiness.card), expected };
  const decided = await evaluate(
    cdp,
    resolvePermissionExpression(
      focusedRememberedChatSurfacePreamble(
        chat,
        'return {outcome:"scope-lost"};',
      ),
      turnId,
      decision,
      identity,
    ),
    permissionDeadline,
  );
  if (decided?.outcome !== "resolved") {
    observations.steps.push({
      name: `${label}-permission-decision-refused`,
      value: { decided, decision, identity },
    });
    await snapshot(cdp, chat, `${label}-permission-decision-refused`, turnId);
    throw new Error(
      `hr6-permission-decision-${decided?.outcome ?? "unreadable"}-${label}`,
    );
  }
  let outcome = null;
  let pendingSamples = 0;
  while (Date.now() < permissionDeadline) {
    state = await freshState(
      cdp,
      chat,
      stateExpression,
      permissionDeadline,
      label,
      "permission-decision-read",
      turnId,
    );
    outcome = permissionOutcome(state, decision, identity);
    if (outcome?.kind === "permission-terminal") break;
    if (outcome && PERMISSION_PENDING_KINDS.includes(outcome.kind)) {
      pendingSamples += 1;
      await pollYield(permissionDeadline);
      continue;
    }
    if (outcome) {
      observations.steps.push({
        name: `${label}-permission-${outcome.kind}`,
        value: { state, outcome, pendingSamples },
      });
      await snapshot(cdp, chat, `${label}-permission-${outcome.kind}`, turnId);
      throw new Error(`hr6-permission-${outcome.kind}-${label}`);
    }
    await pollYield(permissionDeadline);
  }
  if (outcome?.kind !== "permission-terminal") {
    observations.steps.push({
      name: `${label}-permission-decision-timeout`,
      value: { state, outcome, pendingSamples },
    });
    await snapshot(cdp, chat, `${label}-permission-decision-timeout`, turnId);
    throw new Error(`hr6-timeout-${label}-permission-decision`);
  }
  observations.steps.push({
    name: `${label}-permission-resolved`,
    value: {
      card: outcome.card,
      action: decided.action,
      decision,
      pendingSamples,
    },
  });
  await snapshot(cdp, chat, `${label}-permission-resolved`, turnId);
  return outcome.card;
}

async function assertFileChange(cdp, chat, turnId, expected, label) {
  const changeDeadline = Math.min(deadline, Date.now() + 45_000);
  const stateExpression = fileChangesExpression(
    focusedRememberedChatSurfacePreamble(chat, 'return {kind:"scope-lost"};'),
    turnId,
  );
  let outcome = null;
  let state = null;
  while (Date.now() < changeDeadline) {
    state = await freshState(
      cdp,
      chat,
      stateExpression,
      changeDeadline,
      label,
      "file-change-read",
      turnId,
    );
    outcome = fileChangeOutcome(state, expected);
    if (outcome?.kind === "file-change") break;
    if (outcome) {
      observations.steps.push({
        name: `${label}-${outcome.kind}`,
        value: { state, outcome, expected },
      });
      await snapshot(cdp, chat, `${label}-${outcome.kind}`, turnId);
      throw new Error(`hr6-${outcome.kind}-${label}`);
    }
    await pollYield(changeDeadline);
  }
  if (outcome?.kind !== "file-change") {
    observations.steps.push({
      name: `${label}-file-change-timeout`,
      value: { state, outcome, expected },
    });
    await snapshot(cdp, chat, `${label}-file-change-timeout`, turnId);
    throw new Error(`hr6-timeout-${label}-file-change`);
  }
  observations.steps.push({
    name: `${label}-file-change`,
    value: { entry: outcome.entry, sections: state.sections },
  });
  await snapshot(cdp, chat, `${label}-file-change`, turnId);
  return outcome.entry;
}

async function openDiffArtifact(cdp, chat, turnId, expected, label) {
  const opened = await evaluate(
    cdp,
    openDiffExpression(
      focusedRememberedChatSurfacePreamble(
        chat,
        'return {outcome:"scope-lost"};',
      ),
      turnId,
      expected,
    ),
  );
  if (opened?.outcome !== "opened") {
    observations.steps.push({
      name: `${label}-open-diff-refused`,
      value: { opened, expected },
    });
    await snapshot(cdp, chat, `${label}-open-diff-refused`, turnId);
    throw new Error(
      `hr6-open-diff-${opened?.outcome ?? "unreadable"}-${label}`,
    );
  }
  const surfaceDeadline = Math.min(deadline, Date.now() + 45_000);
  const stateExpression = diffSurfaceExpression();
  let outcome = null;
  let state = null;
  let pendingSamples = 0;
  while (Date.now() < surfaceDeadline) {
    state = await freshState(
      cdp,
      chat,
      stateExpression,
      surfaceDeadline,
      label,
      "diff-surface-read",
      turnId,
    );
    outcome = diffSurfaceOutcome(state, expected);
    if (outcome?.kind === "diff-surface") break;
    if (outcome && DIFF_PENDING_KINDS.includes(outcome.kind)) {
      pendingSamples += 1;
      await pollYield(surfaceDeadline);
      continue;
    }
    if (outcome) {
      observations.steps.push({
        name: `${label}-${outcome.kind}`,
        value: { state, outcome, expected, pendingSamples },
      });
      await snapshot(cdp, chat, `${label}-${outcome.kind}`, turnId);
      throw new Error(`hr6-${outcome.kind}-${label}`);
    }
    await pollYield(surfaceDeadline);
  }
  if (outcome?.kind !== "diff-surface") {
    observations.steps.push({
      name: `${label}-diff-surface-timeout`,
      value: { state, outcome, expected, pendingSamples },
    });
    await snapshot(cdp, chat, `${label}-diff-surface-timeout`, turnId);
    throw new Error(`hr6-timeout-${label}-diff-surface`);
  }
  observations.steps.push({
    name: `${label}-diff-surface`,
    value: { state, lines: outcome.lines, pendingSamples },
  });
  await shot(cdp, `${label}-artifact-pane`);
  await snapshot(cdp, chat, `${label}-diff-surface`, turnId);
  return outcome.lines;
}

async function submit(
  cdp,
  chat,
  prompt,
  label,
  cancel,
  expectedReply,
  requireToolResult = false,
  options = {},
) {
  await waitForSubmissionReadiness(cdp, chat, label);
  markClaudePhase(label, "fill");
  await click(
    cdp,
    `${focusedRememberedChatSurfacePreamble(chat, "return false;")}const one=${boundedFirstProjection()};const scopeOf=${boundedScopeProjection()};const x=one(scopeOf(surface,".composer"),'textarea[aria-label="Message agent"]');if(!x)return false;const set=Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype,'value').set;set.call(x,${JSON.stringify(prompt)});x.dispatchEvent(new Event('input',{bubbles:true}));return true;`,
    `${label}-fill`,
  );
  markClaudePhase(label, "composer");
  await waitFor(
    cdp,
    expression(`
      ${focusedRememberedChatSurfacePreamble(chat, "return false;")}
      const one=${boundedFirstProjection()};
      const scopeOf=${boundedScopeProjection()};
      const composerScope=scopeOf(surface,".composer");
      const input=one(composerScope,'textarea[aria-label="Message agent"]');
      const send=one(composerScope,'button[aria-label="Send message"]');
      return input?.value===${JSON.stringify(prompt)}&&send?.disabled===false;
    `),
    `${label}-composer`,
  );
  const initialAcceptance = await turnAcceptanceSample(cdp, chat);
  const initialTurnCount = initialAcceptance.right?.turnCount;
  if (!Number.isInteger(initialTurnCount))
    throw new Error(`hr6-turn-baseline-unavailable-${label}`);
  initialAcceptance.expectedPane = chat;
  markClaudePhase(label, "send");
  await click(
    cdp,
    `${focusedRememberedChatSurfacePreamble(chat, "return false;")}const one=${boundedFirstProjection()};const scopeOf=${boundedScopeProjection()};const x=one(scopeOf(surface,".composer"),'button[aria-label="Send message"]');if(!x||x.disabled)return false;x.click();return true;`,
    `${label}-send`,
  );
  markClaudePhase(label, "accepted");
  await waitForAcceptedTurn(cdp, chat, initialAcceptance, label);
  const currentTurnId = await evaluate(
    cdp,
    expression(`
      ${focusedRememberedChatSurfacePreamble(chat, "return null;")}
      const turnsOf=${boundedTurnsProjection()};
      const identity=${boundedIdentityProjection()};
      const scan=turnsOf(surface);
      const turn=scan.exceeded?null:scan.turns[${initialTurnCount}];
      return identity(turn?.dataset.turnId);
    `),
  );
  if (!currentTurnId) throw new Error(`hr6-turn-id-unavailable-${label}`);
  if (options.permission)
    await resolvePermission(
      cdp,
      chat,
      currentTurnId,
      options.permission,
      options.permissionExpected,
      label,
    );
  let observedRunningTools = null;
  if (cancel) {
    await waitFor(
      cdp,
      expression(`
        ${focusedRememberedChatSurfacePreamble(chat, "return false;")}
        const scopeOf=${boundedScopeProjection()};
        const find=${boundedMatchProjection(2)};
        const controls=find(scopeOf(surface,".composer"),'button[aria-label="Cancel turn"]',false);
        return controls.nodes.length===1&&controls.nodes[0].disabled===false;
      `),
      `${label}-cancel-ready`,
    );
    observedRunningTools = await cancelRunningTool(
      cdp,
      chat,
      currentTurnId,
      label,
      options.cancelWindowMs ?? 60_000,
      options.cancelTolerated ?? [],
    );
  }
  await snapshot(cdp, chat, `${label}-streaming`, currentTurnId);
  const actionDeadline = Math.min(
    deadline,
    Date.now() + (requireToolResult ? 120000 : 45000),
  );
  let terminal = null;
  let outcome = null;
  let toolEvidence = null;
  let cancelEvidence = null;
  let graceStartedAt = null;
  let graceWaitedMs = 0;
  while (
    Date.now() <
    toolTerminalDeadline(
      actionDeadline,
      deadline,
      graceStartedAt,
      TOOL_TERMINAL_GRACE_MS,
    )
  ) {
    const turnDeadline = toolTerminalDeadline(
      actionDeadline,
      deadline,
      graceStartedAt,
      TOOL_TERMINAL_GRACE_MS,
    );
    await expandToolRollups(cdp, chat, currentTurnId, turnDeadline);
    if (Date.now() >= turnDeadline) break;
    const read = await probeRead(
      cdp,
      terminalExpression(
        focusedRememberedChatSurfacePreamble(chat, "return null;"),
      ),
      turnDeadline,
    );
    if (read.kind === "error") throw read.error;
    if (read.kind === "expired") break;
    if (read.kind === "unresponsive") {
      observations.steps.push({
        name: `${label}-terminal-unresponsive`,
        value: { outcome, graceWaitedMs },
      });
      throw new Error(`hr6-unresponsive-${label}-terminal`);
    }
    terminal = read.value;
    if (!terminal) throw new Error(`hr6-focused-chat-unavailable-${label}`);
    outcome = turnOutcome(terminal, currentTurnId, cancel, expectedReply);
    if (
      outcome?.kind === "error" ||
      outcome?.kind === "cancel-refused" ||
      outcome?.kind === "response-mismatch" ||
      outcome?.kind === "scan-exceeded" ||
      outcome?.kind === "turn-ambiguous"
    ) {
      observations.steps.push({
        name: `${label}-terminal-error`,
        value: { terminal, outcome },
      });
      await snapshot(cdp, chat, `${label}-terminal-error`);
      throw new Error(`hr6-turn-${outcome.kind}-${label}`);
    }
    if (requireToolResult) {
      toolEvidence = toolEvidenceOutcome(terminal, currentTurnId, READ_PROBE);
      if (toolEvidence && toolEvidence.kind !== "tool-result") {
        observations.steps.push({
          name: `${label}-tool-evidence-error`,
          value: { terminal, outcome, toolEvidence },
        });
        await snapshot(cdp, chat, `${label}-tool-evidence-error`);
        throw new Error(`hr6-tool-evidence-${toolEvidence.kind}-${label}`);
      }
      if (outcome && toolEvidence) break;
    } else if (cancel) {
      cancelEvidence = cancelledToolOutcome(
        terminal,
        currentTurnId,
        observedRunningTools,
        options.cancelTerminalStates ?? null,
      );
      const grace = toolTerminalGrace(
        cancelEvidence,
        graceStartedAt,
        Date.now(),
      );
      graceStartedAt = grace.startedAt;
      graceWaitedMs = grace.waitedMs;
      if (grace.kind === "grace") {
        await pollYield(turnDeadline);
        continue;
      }
      if (cancelEvidence && cancelEvidence.kind !== "tool-terminal") {
        observations.steps.push({
          name: `${label}-cancel-tool-error`,
          value: { terminal, outcome, cancelEvidence, graceWaitedMs },
        });
        await snapshot(cdp, chat, `${label}-cancel-tool-error`, currentTurnId);
        throw new Error(`hr6-cancel-tool-${cancelEvidence.kind}-${label}`);
      }
      if (outcome && cancelEvidence) break;
    } else if (outcome) break;
    await pollYield(turnDeadline);
  }
  if (!outcome) throw new Error(`hr6-timeout-${label}-terminal`);
  if (cancel && cancelEvidence?.kind !== "tool-terminal") {
    observations.steps.push({
      name: `${label}-cancel-tool-timeout`,
      value: { terminal, outcome, cancelEvidence, graceWaitedMs },
    });
    await snapshot(cdp, chat, `${label}-cancel-tool-timeout`, currentTurnId);
    throw new Error(`hr6-timeout-${label}-cancel-tool-terminal`);
  }
  if (requireToolResult && toolEvidence?.kind !== "tool-result") {
    observations.steps.push({
      name: `${label}-tool-evidence-timeout`,
      value: { terminal, outcome, toolEvidence },
    });
    await snapshot(cdp, chat, `${label}-tool-evidence-timeout`);
    throw new Error(`hr6-timeout-${label}-tool-evidence`);
  }
  observations.steps.push({
    name: `${label}-wait-result`,
    value: { terminal, outcome, toolEvidence, cancelEvidence, graceWaitedMs },
  });
  await snapshot(cdp, chat, `${label}-terminal`, currentTurnId);
  return currentTurnId;
}

async function cancelRunningTool(
  cdp,
  chat,
  turnId,
  label,
  windowMs = 60_000,
  tolerated = [],
) {
  const runningDeadline = Math.min(deadline, Date.now() + windowMs);
  const attempts = [];
  let readiness = null;
  let state = null;
  let rollupRescans = 0;
  let toleratedSamples = 0;
  while (Date.now() < runningDeadline) {
    await expandToolRollups(cdp, chat, turnId, runningDeadline);
    state = await evaluate(
      cdp,
      terminalExpression(
        focusedRememberedChatSurfacePreamble(chat, "return null;"),
      ),
      runningDeadline,
    );
    if (!state) throw new Error(`hr6-focused-chat-unavailable-${label}`);
    readiness = runningToolReadiness(state, turnId);
    const toleratedKind =
      readiness !== null &&
      readiness.kind !== "tool-running" &&
      tolerated.includes(readiness.kind);
    if (toleratedKind) toleratedSamples += 1;
    if (readiness && readiness.kind !== "tool-running" && !toleratedKind) {
      observations.steps.push({
        name: `${label}-cancel-tool-${readiness.kind}`,
        value: { terminal: state, readiness, attempts },
      });
      await snapshot(
        cdp,
        chat,
        `${label}-cancel-tool-${readiness.kind}`,
        turnId,
      );
      throw new Error(`hr6-cancel-tool-${readiness.kind}-${label}`);
    }
    if (readiness?.kind === "tool-running") {
      const tool = readiness.tools[0];
      const observedAt = Date.now();
      const result = await evaluate(
        cdp,
        cancelRunningToolExpression(
          focusedRememberedChatSurfacePreamble(
            chat,
            'return {outcome:"scope-lost"};',
          ),
          turnId,
          tool.id,
        ),
        runningDeadline,
      );
      if (attempts.length < TERMINAL_LIMITS.tools)
        attempts.push({
          tool,
          outcome: result?.outcome ?? null,
          state: result?.state ?? null,
          observedAtMs: observedAt - runnerStartedAt,
          settledAtMs: Date.now() - runnerStartedAt,
        });
      if (result?.outcome === "scope-lost")
        throw new Error(`hr6-focused-chat-unavailable-${label}`);
      if (
        result?.outcome === "turn-superseded" ||
        result?.outcome === "turn-ambiguous" ||
        result?.outcome === "turn-scan-exceeded" ||
        result?.outcome === "control-scan-exceeded" ||
        result?.outcome === "composer-unresolved" ||
        result?.outcome === "tool-scan-exceeded" ||
        result?.outcome === "tool-card-ambiguous"
      ) {
        observations.steps.push({
          name: `${label}-cancel-tool-${result.outcome}`,
          value: { terminal: state, tool, attempts },
        });
        await snapshot(
          cdp,
          chat,
          `${label}-cancel-tool-${result.outcome}`,
          turnId,
        );
        throw new Error(`hr6-cancel-tool-${result.outcome}-${label}`);
      }
      if (result?.outcome === "cancelled") {
        observations.steps.push({
          name: `${label}-cancel-tool-running`,
          value: { tool, attempts, rollupRescans, toleratedSamples },
        });
        await snapshot(cdp, chat, `${label}-cancel-tool-running`, turnId);
        return [tool];
      }
    }
    if (
      !readiness &&
      rollupRescans < TERMINAL_LIMITS.tools &&
      rollupRescanNeeded(state, turnId)
    ) {
      rollupRescans += 1;
      continue;
    }
    await pollYield(runningDeadline);
  }
  observations.steps.push({
    name: `${label}-cancel-tool-running-timeout`,
    value: {
      terminal: state,
      readiness,
      attempts,
      rollupRescans,
      toleratedSamples,
    },
  });
  await snapshot(cdp, chat, `${label}-cancel-tool-running-timeout`, turnId);
  throw new Error(`hr6-timeout-${label}-cancel-tool-running`);
}

async function waitForSubmissionReadiness(cdp, chat, label) {
  let readiness = null;
  while (Date.now() < deadline) {
    readiness = await evaluate(
      cdp,
      expression(
        `${focusedRememberedChatSurfacePreamble(chat, "return null;")}const one=${boundedFirstProjection()};const scopeOf=${boundedScopeProjection()};const findAlerts=${boundedMatchProjection(TERMINAL_LIMITS.alerts)};const walk=${boundedNodeTextProjection(160)};const head=${boundedHeadProjection()};const input=one(scopeOf(surface,".composer"),'textarea[aria-label="Message agent"]');const alerts=findAlerts(surface,'[role="alert"]',false,"section.acp-turn");return input?{alerts:head(alerts.nodes,${TERMINAL_LIMITS.alerts},x=>walk(x).text),alertsExceeded:alerts.exceeded,disabled:input.disabled,status:walk(one(surface,'.session-state')).text||null}:null;`,
      ),
    );
    const state = submissionReadiness(readiness);
    if (state === "ready") {
      observations.steps.push({ name: `${label}-readiness`, value: readiness });
      return readiness;
    }
    if (state === "persistent" || state === "error") {
      observations.steps.push({
        name: `${label}-readiness-${state}`,
        value: readiness,
      });
      await snapshot(cdp, chat, `${label}-blocked`);
      throw new Error(`hr6-submission-${state}-${label}`);
    }
    await pollYield();
  }
  observations.steps.push({
    name: `${label}-readiness-timeout`,
    value: readiness,
  });
  await snapshot(cdp, chat, `${label}-blocked`);
  throw new Error(`hr6-timeout-${label}-readiness`);
}

async function settleBeforeHarnessSwitch(cdp, chat, label) {
  let state = await evaluate(
    cdp,
    expression(`
      ${focusedRememberedChatSurfacePreamble(chat, "return null;")}
      const one=${boundedFirstProjection()};
      const scopeOf=${boundedScopeProjection()};
      const composerScope=scopeOf(surface,".composer");
      const composer=one(composerScope,'textarea[aria-label="Message agent"]');
      const cancel=one(composerScope,'button[aria-label="Cancel turn"]');
      return {composerDisabled:composer?.disabled??null,cancelVisible:Boolean(cancel)};
    `),
  );
  if (!state) throw new Error(`hr6-focused-chat-unavailable-${label}`);
  if (state.cancelVisible) {
    const cancelled = await evaluate(
      cdp,
      expression(`
        ${focusedRememberedChatSurfacePreamble(chat, "return false;")}
        const scopeOf=${boundedScopeProjection()};
        const find=${boundedMatchProjection(2)};
        const controls=find(scopeOf(surface,".composer"),'button[aria-label="Cancel turn"]',false);
        if(controls.nodes.length!==1||controls.nodes[0].disabled)return false;
        controls.nodes[0].click();return true;
      `),
    );
    observations.steps.push({ name: `${label}-cancel`, value: cancelled });
    if (cancelled !== true) throw new Error(`hr6-cancel-refused-${label}`);
  }
  if (!turnSettled(state)) {
    await waitFor(
      cdp,
      expression(`
        ${focusedRememberedChatSurfacePreamble(chat, "return false;")}
        const one=${boundedFirstProjection()};
        const scopeOf=${boundedScopeProjection()};
        const composerScope=scopeOf(surface,".composer");
        const composer=one(composerScope,'textarea[aria-label="Message agent"]');
        const cancel=one(composerScope,'button[aria-label="Cancel turn"]');
        return composer?.disabled===false&&!cancel;
      `),
      `${label}-settled`,
    );
    state = await evaluate(
      cdp,
      expression(`
        ${focusedRememberedChatSurfacePreamble(chat, "return null;")}
        const one=${boundedFirstProjection()};
        const scopeOf=${boundedScopeProjection()};
        const composerScope=scopeOf(surface,".composer");
        const composer=one(composerScope,'textarea[aria-label="Message agent"]');
        const cancel=one(composerScope,'button[aria-label="Cancel turn"]');
        return {composerDisabled:composer?.disabled??null,cancelVisible:Boolean(cancel)};
      `),
    );
    if (!state) throw new Error(`hr6-focused-chat-unavailable-${label}`);
  }
  observations.steps.push({ name: `${label}-settled`, value: state });
  await snapshot(cdp, chat, `${label}-settled`);
}

async function createAgentChat(cdp, label) {
  const tabCount = await evaluate(
    cdp,
    expression(
      `const find=${boundedMatchProjection(TERMINAL_LIMITS.tools)};return find(document.documentElement,'[role="tab"][aria-controls="agent-chat-panel"]',false,"section.acp-turn").nodes.length;`,
    ),
  );
  await click(
    cdp,
    `const one=${boundedFirstProjection()};const x=one(document.documentElement,'button[aria-label="New agent chat"]');if(!x||x.disabled)return false;x.click();return true;`,
    `${label}-create`,
  );
  await waitFor(
    cdp,
    expression(`
      const find=${boundedMatchProjection(TERMINAL_LIMITS.tools)};
      const walk=${boundedNodeTextProjection(160)};
      const tabs=find(document.documentElement,'[role="tab"][aria-controls="agent-chat-panel"]',false,"section.acp-turn").nodes;
      return tabs.length>${tabCount}&&tabs.some(x=>x.getAttribute('aria-selected')==='true'&&/New chat/u.test(walk(x).text));
    `),
    `${label}-created`,
  );
}

let cdp;
try {
  const target = await discoverTarget(port, "http://127.0.0.1:1420/", deadline);
  cdp = await attachCdp(target, deadline);
  await waitFor(
    cdp,
    expression(
      `const one=${boundedFirstProjection()};const agent=one(document.documentElement,'input[name="workbench-window-mode"][value="agent"]');if(!agent)return false;if(!agent.checked)agent.click();return true;`,
    ),
    "agent-mode",
  );
  await waitFor(
    cdp,
    expression(`${focusedChatSurfacePreamble("return false;")}return true;`),
    "chat-surface",
  );
  await rememberFocusedChat(cdp, "codex", "01-harness-discovery");
  await snapshot(cdp, "codex", "01-harness-discovery");
  const codex = await selectHarness(cdp, "codex", "Codex", "02-codex");
  if (!codex) throw new Error("hr6-harness-unavailable-02-codex");
  await submit(
    cdp,
    "codex",
    "Reply with the single word PONG.",
    "03-codex-pong",
    false,
    /^PONG[.!]?$/u,
  );
  await assertReportedUsage(cdp, "codex", "03-codex-pong");
  await submit(
    cdp,
    "codex",
    TOOL_RESULT_PROMPT,
    "04-codex-tool-result",
    false,
    undefined,
    true,
  );
  await submit(cdp, "codex", TOOL_CANCEL_PROMPT, "04-codex-tool-cancel", true);
  const codexTurnIds = await rememberTurnIds(cdp, "codex", "04-codex");
  await settleBeforeHarnessSwitch(cdp, "codex", "04-codex-before-claude");
  await createAgentChat(cdp, "05-claude-chat");
  await rememberFocusedChat(cdp, "claude", "05-claude-chat");
  await snapshot(cdp, "claude", "05-claude-chat-created");
  await settleBeforeHarnessSwitch(cdp, "claude", "05-claude-chat");
  const claude = await selectHarness(cdp, "claude", "Claude", "05-claude");
  if (!claude) throw new Error("hr6-harness-unavailable-05-claude");
  await assertRememberedHarness(cdp, "codex", "Codex", "06-codex");
  await focusRememberedChat(cdp, "codex", "06-codex");
  await assertRememberedTurnContinuity(cdp, "codex", codexTurnIds, "06-codex");
  await submit(
    cdp,
    "codex",
    "Reply with the single word PONG.",
    "06-codex-restart-pong",
    false,
    /^PONG[.!]?$/u,
  );
  await focusRememberedChat(cdp, "claude", "07-claude");
  await assertRememberedHarness(cdp, "claude", "Claude", "07-claude");
  await submit(
    cdp,
    "claude",
    "Reply with the single word PONG.",
    "07-claude-pong",
    false,
    /^PONG[.!]?$/u,
  );
  await assertReportedUsage(cdp, "claude", "07-claude-pong");
  await submit(
    cdp,
    "claude",
    TOOL_RESULT_PROMPT,
    "08-claude-tool-result",
    false,
    undefined,
    true,
  );
  await selectAccessMode(
    cdp,
    "claude",
    APPROVAL_ACCESS_MODE,
    "08b-claude-access-mode",
  );
  const permissionTurnId = await submit(
    cdp,
    "claude",
    PERMISSION_WRITE_PROMPT,
    "09-claude-permission-write",
    false,
    /WROTE/u,
    false,
    { permission: "allowOnce", permissionExpected: PERMISSION_EXPECTED },
  );
  assertScratchFile(
    PERMISSION_FILE,
    PERMISSION_LINE,
    "09-claude-permission-write",
  );
  await assertFileChange(
    cdp,
    "claude",
    permissionTurnId,
    {
      relativePath: PERMISSION_FILE,
      rootName: SCRATCH_ROOT_LABEL,
      additions: 1,
      deletions: 0,
      operation: "created",
    },
    "10-claude-diff",
  );
  await openDiffArtifact(
    cdp,
    "claude",
    permissionTurnId,
    {
      relativePath: PERMISSION_FILE,
      rootName: SCRATCH_ROOT_LABEL,
      additions: 1,
      deletions: 0,
      addedLines: [PERMISSION_LINE],
    },
    "10-claude-diff",
  );
  await submit(
    cdp,
    "claude",
    PERMISSION_DECLINE_PROMPT,
    "11-claude-permission-decline",
    false,
    undefined,
    false,
    { permission: "deny", permissionExpected: DECLINE_EXPECTED },
  );
  assertScratchAbsent(DECLINE_FILE, "11-claude-permission-decline");
  await submit(
    cdp,
    "claude",
    LONG_TOOL_PROMPT,
    "12-claude-long-tool-cancel",
    true,
    undefined,
    false,
    {
      permission: "allowOnce",
      permissionExpected: LONG_TOOL_EXPECTED,
      cancelWindowMs: 90_000,
      cancelTolerated: ["tool-awaiting-approval"],
      cancelTerminalStates: ["cancelled"],
    },
  );
  assertScratchAbsent(LONG_TOOL_RELEASE_FILE, "12-claude-long-tool-cancel");
  if (includeOpenCode) {
    await settleBeforeHarnessSwitch(cdp, "claude", "12-claude-before-opencode");
    await createAgentChat(cdp, "13-opencode-chat");
    await rememberFocusedChat(cdp, "opencode", "13-opencode-chat");
    await snapshot(cdp, "opencode", "13-opencode-chat-created");
    await settleBeforeHarnessSwitch(cdp, "opencode", "13-opencode-chat");
    const selection = {};
    const opencode = await selectHarness(
      cdp,
      "opencode",
      "OpenCode",
      "13-opencode",
      selection,
    );
    if (!opencode) {
      const outcome = harnessSelectionOutcome(selection.state);
      observations.steps.push({
        name: "13-opencode-outcome",
        value: outcome,
      });
      await snapshot(cdp, "opencode", "13-opencode-outcome");
      throw new Error(`hr6-${outcome.kind}-13-opencode-pong`);
    }
    await submit(
      cdp,
      "opencode",
      "Reply with the single word PONG.",
      "13-opencode-pong",
      false,
      /^PONG[.!]?$/u,
    );
  }
  observations.completedAt = new Date().toISOString();
  observations.cdp = cdp.stats;
  writeJson("observations", observations);
} catch (error) {
  observations.failedAt = new Date().toISOString();
  observations.error = String(error?.message ?? error).slice(0, 2048);
  if (cdp) observations.cdp = cdp.stats;
  writeJson("observations", observations);
  throw error;
} finally {
  if (cdp) await cdp.close(Date.now() + 5000).catch(() => {});
}
