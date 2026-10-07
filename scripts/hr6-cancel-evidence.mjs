import { scanExceeded, turnSettled } from "./hr6-readiness.mjs";
import {
  TERMINAL_LIMITS,
  boundedMatchProjection,
  boundedScalarProjection,
  boundedTurnsProjection,
} from "./hr6-terminal-expression.mjs";

export const RUNNING_TOOL_STATE = "inProgress";
export const TERMINAL_TOOL_STATES = [
  "completed",
  "failed",
  "declined",
  "cancelled",
];
const APPROVAL_TOOL_STATE = "waitingForApproval";
export const TOOL_TERMINAL_GRACE_MS = 8000;

export function toolIdentity(tool) {
  const id = tool?.id;
  return typeof id === "string" && id.length > 0 ? id : null;
}

function turnTools(state, turnId) {
  const matches = Array.isArray(state?.turns)
    ? state.turns.filter((candidate) => candidate?.id === turnId)
    : [];
  if (matches.length > 1) return { ambiguous: true };
  const turn = matches[0];
  if (!turn || typeof turn.status !== "string") return null;
  return { turn, tools: Array.isArray(turn.tools) ? turn.tools : [] };
}

function describe(tool) {
  return {
    id: toolIdentity(tool),
    kind: typeof tool?.kind === "string" ? tool.kind : null,
    label: typeof tool?.label === "string" ? tool.label : null,
    state: typeof tool?.state === "string" ? tool.state : null,
  };
}

export function runningTools(state, turnId) {
  const found = turnTools(state, turnId);
  if (!found || found.ambiguous === true) return [];
  return found.tools.filter(
    (tool) => tool?.state === RUNNING_TOOL_STATE && toolIdentity(tool) !== null,
  );
}

function turnRollups(found) {
  return Array.isArray(found?.turn?.rollups) ? found.turn.rollups : [];
}

export function runningRollups(state, turnId) {
  const found = turnTools(state, turnId);
  if (!found || found.ambiguous === true) return [];
  return turnRollups(found).filter(
    (candidate) => candidate?.state === RUNNING_TOOL_STATE,
  );
}

export function rollupRescanNeeded(state, turnId) {
  const found = turnTools(state, turnId);
  if (!found || found.ambiguous === true) return false;
  if (turnSettled(state)) return false;
  if (scanExceeded(state, found.turn)) return false;
  if (runningTools(state, turnId).length > 0) return false;
  return runningRollups(state, turnId).some(
    (candidate) => candidate.open !== true || candidate.cards === 0,
  );
}

export function runningToolReadiness(state, turnId) {
  const found = turnTools(state, turnId);
  if (!found) return null;
  if (found.ambiguous === true) return { kind: "turn-ambiguous" };
  if (state.alerts?.length > 0) return { kind: "error" };
  if (scanExceeded(state, found.turn))
    return { kind: "scan-exceeded", tools: found.tools.map(describe) };
  const running = runningTools(state, turnId);
  if (turnSettled(state))
    return {
      kind: found.tools.length === 0 ? "tool-absent" : "turn-settled",
      tools: found.tools.map(describe),
    };
  if (running.length === 0) {
    if (found.tools.some((tool) => tool?.state === RUNNING_TOOL_STATE))
      return {
        kind: "tool-card-unidentified",
        tools: found.tools.map(describe),
      };
    if (found.tools.some((tool) => tool?.state === APPROVAL_TOOL_STATE))
      return {
        kind: "tool-awaiting-approval",
        tools: found.tools.map(describe),
      };
    return null;
  }
  return { kind: "tool-running", tools: running.map(describe) };
}

function requiredToolStates(required) {
  if (required === null || required === undefined) return null;
  if (!Array.isArray(required) || required.length === 0)
    throw new Error("hr6-cancel-required-states-invalid");
  if (
    required.some(
      (value) =>
        typeof value !== "string" || !TERMINAL_TOOL_STATES.includes(value),
    )
  )
    throw new Error("hr6-cancel-required-states-invalid");
  return required;
}

export function cancelledToolOutcome(state, turnId, observed, required = null) {
  const demanded = requiredToolStates(required);
  const found = turnTools(state, turnId);
  if (!found) return null;
  if (found.ambiguous === true) return { kind: "turn-ambiguous" };
  if (state.alerts?.length > 0) return { kind: "error" };
  if (scanExceeded(state, found.turn))
    return { kind: "scan-exceeded", tools: found.tools.map(describe) };
  const identities = Array.isArray(observed) ? observed.map(toolIdentity) : [];
  if (identities.length === 0 || identities.some((id) => id === null))
    return { kind: "observation-missing" };
  if (!turnSettled(state)) return null;
  const wanted = new Set(identities);
  const matched = found.tools.filter((tool) => wanted.has(toolIdentity(tool)));
  if (new Set(matched.map(toolIdentity)).size !== wanted.size)
    return { kind: "tool-card-lost", tools: found.tools.map(describe) };
  if (matched.length !== wanted.size)
    return { kind: "tool-card-ambiguous", tools: matched.map(describe) };
  const unsettled = found.tools.filter(
    (tool) => !TERMINAL_TOOL_STATES.includes(tool?.state),
  );
  if (unsettled.length > 0)
    return { kind: "tool-not-terminal", tools: unsettled.map(describe) };
  if (
    demanded !== null &&
    matched.some((tool) => !demanded.includes(tool?.state))
  )
    return { kind: "tool-not-cancelled", tools: matched.map(describe) };
  return { kind: "tool-terminal", tools: matched.map(describe) };
}

export function toolTerminalGrace(
  evidence,
  startedAt,
  now,
  graceMs = TOOL_TERMINAL_GRACE_MS,
) {
  if (evidence?.kind !== "tool-not-terminal")
    return { kind: "other", startedAt: null, waitedMs: 0 };
  const since = Number.isFinite(startedAt) ? startedAt : now;
  const waitedMs = Math.max(0, now - since);
  if (waitedMs < graceMs) return { kind: "grace", startedAt: since, waitedMs };
  return { kind: "expired", startedAt: since, waitedMs };
}

export function cancelRunningToolExpression(preamble, turnId, toolId) {
  return `(()=>{${preamble}
    const turns=${boundedTurnsProjection()};
    const find=${boundedMatchProjection(TERMINAL_LIMITS.tools)};
    const controls=${boundedMatchProjection(2)};
    const scalar=${boundedScalarProjection()};
    const turnScan=turns(surface);
    if(turnScan.exceeded)return {outcome:"turn-scan-exceeded"};
    const scoped=turnScan.turns;
    let target=null;
    let matches=0;
    for(let index=scoped.length-1;index>=0;index-=1){
      const candidate=scoped[index];
      if(candidate.dataset.turnId===${JSON.stringify(turnId)}){matches+=1;if(!target)target=candidate;}
    }
    if(matches===0)return {outcome:"turn-lost"};
    if(matches>1)return {outcome:"turn-ambiguous"};
    if(scoped[scoped.length-1]!==target)return {outcome:"turn-superseded"};
    const scan=find(target,".acp-tool-card",true);
    if(scan.exceeded)return {outcome:"tool-scan-exceeded"};
    const cards=scan.nodes;
    let card=null;
    let cardMatches=0;
    for(let index=0;index<cards.length;index+=1){
      if(cards[index].getAttribute("data-acp-item-id")===${JSON.stringify(toolId)}){cardMatches+=1;if(!card)card=cards[index];}
    }
    if(cardMatches===0)return {outcome:"tool-card-lost"};
    if(cardMatches>1)return {outcome:"tool-card-ambiguous"};
    const raw=card.getAttribute("data-activity-state");
    const state=scalar(raw);
    if(raw!==${JSON.stringify(RUNNING_TOOL_STATE)})return {outcome:"tool-not-running",state};
    const composerScan=controls(surface,".composer",false);
    if(composerScan.exceeded!==false||composerScan.nodes.length!==1)return {outcome:"composer-unresolved",state};
    const controlScan=controls(composerScan.nodes[0],'button[aria-label="Cancel turn"]',false);
    if(controlScan.exceeded)return {outcome:"control-scan-exceeded",state};
    const buttons=controlScan.nodes;
    if(buttons.length!==1||buttons[0].disabled)return {outcome:"cancel-unavailable",state};
    buttons[0].click();
    return {outcome:"cancelled",state};
  })()`;
}
