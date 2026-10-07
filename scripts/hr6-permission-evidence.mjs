import {
  TERMINAL_LIMITS,
  boundedFirstProjection,
  boundedIdentityProjection,
  boundedMatchProjection,
  boundedNodeTextProjection,
  boundedScalarProjection,
  boundedTurnsProjection,
} from "./hr6-terminal-expression.mjs";

export const PERMISSION_LIMIT = 4;
export const AWAITING_PERMISSION = "Waiting for approval";
export const ALLOWED_PERMISSION = "Permission allowed once";
export const DECLINED_PERMISSION = "Permission declined";
export const TERMINAL_PERMISSION_STATUSES = [
  ALLOWED_PERMISSION,
  DECLINED_PERMISSION,
  "Permission cancelled",
  "Permission failed closed",
];
const TERMINAL_TURN_STATUSES = [
  "Completed",
  "Interrupted",
  "Cancelled",
  "Failed",
  "Delivery unknown",
];
const DECISIONS = {
  allowOnce: { action: "Approve", status: ALLOWED_PERMISSION },
  deny: { action: "Deny", status: DECLINED_PERMISSION },
};
const ACTION_TEXT_LIMIT = 32;
const FRAGMENT_LIMIT = 8;
const FRAGMENT_ANCHOR = "hr6";
const SUMMARY_CHILD_LIMIT = 16;
const COMMAND_LIMIT = 12;
const REFUSED_TOOL_STATE = "failed";
const TOOL_REFUSAL_MARKER = "tool_use_error";

export function permissionDecision(decision) {
  return Object.prototype.hasOwnProperty.call(DECISIONS, decision)
    ? DECISIONS[decision]
    : null;
}

function genericExpectation(value) {
  return value.length < FRAGMENT_LIMIT || value.indexOf(FRAGMENT_ANCHOR) < 0;
}

function normaliseCommand(value) {
  return value.replace(/\s+/gu, " ").trim();
}

function summaryScanSource(cardName) {
  return `const kids=${cardName}.children;
    const kidCount=kids?kids.length:0;
    const kidScan=kidCount>${SUMMARY_CHILD_LIMIT}?${SUMMARY_CHILD_LIMIT}:kidCount;
    let summaryNode=null;
    let summaryCount=0;
    for(let kid=0;kid<kidScan;kid+=1){
      if(kids[kid].tagName!=="P")continue;
      summaryCount+=1;
      if(summaryNode===null)summaryNode=kids[kid];
    }
    const summarySingle=summaryCount===1;
    const detailRead=summarySingle?detail(summaryNode):{text:"",truncated:false};
    const detailText=summarySingle?detailRead.text||null:null;
    const detailTruncated=!summarySingle||detailRead.truncated===true;
    const summaryExceeded=kidCount>${SUMMARY_CHILD_LIMIT}||detailRead.truncated===true;`;
}

function expectationFragments(expected) {
  if (!Array.isArray(expected) || expected.length === 0)
    throw new Error("hr6-permission-expectation-invalid");
  const fragments = expected.map((fragment) =>
    typeof fragment === "string" ? fragment : "",
  );
  if (fragments.some((fragment) => fragment.length === 0))
    throw new Error("hr6-permission-expectation-invalid");
  if (fragments.some(genericExpectation))
    throw new Error("hr6-permission-expectation-generic");
  return fragments;
}

function permissionExpectation(expected) {
  if (Array.isArray(expected))
    return { fragments: expectationFragments(expected), command: null };
  if (!expected || typeof expected !== "object")
    throw new Error("hr6-permission-expectation-invalid");
  if (typeof expected.command !== "string")
    throw new Error("hr6-permission-expectation-invalid");
  const command = normaliseCommand(expected.command);
  if (command.length === 0)
    throw new Error("hr6-permission-expectation-invalid");
  if (command.length < COMMAND_LIMIT)
    throw new Error("hr6-permission-expectation-generic");
  return { fragments: [], command };
}

function matchExpression(expectation, labelName, detailName, truncatedName) {
  if (expectation.command !== null)
    return `((want)=>{
      if(${truncatedName}===true)return false;
      if(typeof ${detailName}!=="string")return false;
      return ${detailName}.replace(/\\s+/gu," ").trim()===want;
    })(${JSON.stringify(expectation.command)})`;
  return `((want)=>{
    for(let cursor=0;cursor<want.length;cursor+=1){
      const inLabel=typeof ${labelName}==="string"&&${labelName}.indexOf(want[cursor])>=0;
      const inDetail=typeof ${detailName}==="string"&&${detailName}.indexOf(want[cursor])>=0;
      if(!inLabel&&!inDetail)return false;
    }
    return want.length>0;
  })(${JSON.stringify(expectation.fragments)})`;
}

export function permissionIdentity(card) {
  return {
    itemId: typeof card?.itemId === "string" ? card.itemId : null,
    label: typeof card?.label === "string" ? card.label : null,
    detail: typeof card?.detail === "string" ? card.detail : null,
  };
}

export function permissionCardsProjection(expected) {
  const expectation = permissionExpectation(expected);
  return `((find,label,detail,scalar,name)=>(node)=>{
    const scan=find(node,".acp-permission",false);
    const out=[];
    for(let index=0;index<scan.nodes.length;index+=1){
      const card=scan.nodes[index];
      const actions=find(card,".acp-permission__action",false);
      let approve=false;
      let deny=false;
      for(let cursor=0;cursor<actions.nodes.length;cursor+=1){
        const action=actions.nodes[cursor];
        const text=name(action).text;
        if(text===${JSON.stringify(DECISIONS.allowOnce.action)}&&action.disabled===false)approve=true;
        if(text===${JSON.stringify(DECISIONS.deny.action)}&&action.disabled===false)deny=true;
      }
      const status=find(card,'[role="status"]',false);
      const single=status.exceeded===false&&status.nodes.length===1;
      const labelText=label(find(card,"strong",false).nodes[0]??null).text||null;
      ${summaryScanSource("card")}
      out.push({
        itemId:scalar(card.getAttribute("data-acp-item-id")),
        label:labelText,
        status:single?scalar(status.nodes[0].getAttribute("aria-label")):null,
        statusExceeded:!single,
        detail:detailText,
        summary:summaryCount,
        summaryExceeded,
        actions:actions.nodes.length,
        actionsExceeded:actions.exceeded,
        approve,
        deny,
        matches:${matchExpression(expectation, "labelText", "detailText", "detailTruncated")}
      });
    }
    return {cards:out,exceeded:scan.exceeded};
  })(${boundedMatchProjection(PERMISSION_LIMIT)},${boundedNodeTextProjection(TERMINAL_LIMITS.label)},${boundedNodeTextProjection()},${boundedScalarProjection()},${boundedNodeTextProjection(ACTION_TEXT_LIMIT)})`;
}

function turnScopeExpression(turnId, onLost, onAmbiguous, onExceeded) {
  return `const turnScan=(${boundedTurnsProjection()})(surface);
    if(turnScan.exceeded){${onExceeded}}
    const scoped=turnScan.turns;
    let target=null;
    let matches=0;
    for(let index=0;index<scoped.length;index+=1){
      if(scoped[index].dataset.turnId===${JSON.stringify(turnId)}){matches+=1;if(!target)target=scoped[index];}
    }
    if(matches===0){${onLost}}
    if(matches>1){${onAmbiguous}}`;
}

export function permissionStateExpression(preamble, turnId, expected) {
  const cards = permissionCardsProjection(expected);
  return `(()=>{${preamble}
    ${turnScopeExpression(
      turnId,
      'return {kind:"turn-lost"};',
      'return {kind:"turn-ambiguous"};',
      'return {kind:"turn-scan-exceeded"};',
    )}
    const one=${boundedFirstProjection()};
    const scalar=${boundedScalarProjection()};
    const scan=(${cards})(target);
    const refusal=(${refusedToolsProjection()})(target);
    return {kind:"permissions",turnStatus:scalar(one(target,"[data-turn-status]")?.getAttribute("aria-label")),cards:scan.cards,exceeded:scan.exceeded,refusals:refusal.tools,refusalsExceeded:refusal.exceeded};
  })()`;
}

function readCards(state) {
  if (!state || typeof state.kind !== "string") return { kind: "read-invalid" };
  if (state.kind !== "permissions") return { kind: state.kind };
  const cards = Array.isArray(state.cards) ? state.cards : [];
  if (state.exceeded !== false)
    return { kind: "permission-scan-exceeded", cards };
  if (
    cards.some(
      (card) =>
        card?.statusExceeded !== false ||
        card?.actionsExceeded !== false ||
        card?.summaryExceeded !== false ||
        card?.summary !== 1,
    )
  )
    return { kind: "permission-unreadable", cards };
  return { cards };
}

export function refusedToolsProjection() {
  return `((find,one,output,label,identity)=>(node)=>{
    const scan=find(node,".acp-tool-card",false);
    const tools=[];
    let exceeded=scan.exceeded!==false;
    for(let index=0;index<scan.nodes.length;index+=1){
      const card=scan.nodes[index];
      if(card.getAttribute("data-activity-state")!==${JSON.stringify(REFUSED_TOOL_STATE)})continue;
      const read=output(one(card,'pre[aria-label="Command output"],pre[aria-label="Tool output"]'));
      if(read.text.indexOf(${JSON.stringify(TOOL_REFUSAL_MARKER)})>=0){
        tools.push({id:identity(card.getAttribute("data-acp-item-id")),label:label(one(card,"strong")).text||null});
        continue;
      }
      if(read.truncated===true)exceeded=true;
    }
    return {tools,exceeded};
  })(${boundedMatchProjection(TERMINAL_LIMITS.tools)},${boundedFirstProjection()},${boundedNodeTextProjection()},${boundedNodeTextProjection(TERMINAL_LIMITS.label)},${boundedIdentityProjection()})`;
}

function refusalReadiness(state, cards) {
  if (!Array.isArray(state.refusals) || state.refusalsExceeded !== false)
    return { kind: "permission-tool-unreadable", cards };
  if (state.refusals.length === 0) return null;
  return { kind: "permission-tool-refused", cards, tools: state.refusals };
}

export function permissionReadiness(state) {
  const read = readCards(state);
  if (read.kind) return read;
  const cards = read.cards;
  if (cards.length > 1) return { kind: "permission-ambiguous", cards };
  const settled = TERMINAL_TURN_STATUSES.includes(state.turnStatus);
  if (cards.length === 0) {
    if (!settled) return null;
    return (
      refusalReadiness(state, cards) ?? { kind: "permission-absent", cards }
    );
  }
  const card = cards[0];
  if (card.matches !== true) return { kind: "permission-unexpected", cards };
  if (
    card.status === AWAITING_PERMISSION &&
    card.approve === true &&
    card.deny === true
  )
    return { kind: "permission-waiting", card, cards };
  return settled ? { kind: "permission-unresolved", cards } : null;
}

export function permissionOutcome(state, decision, identity) {
  const expected = permissionDecision(decision);
  if (!expected) return { kind: "decision-unsupported" };
  const wanted = permissionIdentity(identity);
  if (
    wanted.itemId === null &&
    (wanted.label === null || wanted.detail === null)
  )
    throw new Error("hr6-permission-identity-invalid");
  const read = readCards(state);
  if (read.kind) return read;
  const cards = read.cards;
  if (cards.length === 0) return { kind: "permission-absent" };
  if (cards.length > 1) return { kind: "permission-ambiguous", cards };
  const card = cards[0];
  const identified =
    wanted.itemId !== null
      ? card.itemId === wanted.itemId
      : card.itemId === null &&
        card.label === wanted.label &&
        card.detail === wanted.detail;
  if (!identified) return { kind: "permission-identity-mismatch", cards };
  if (card.matches !== true) return { kind: "permission-unexpected", cards };
  if (card.actions !== 0 || card.approve === true || card.deny === true)
    return { kind: "permission-actionable", cards };
  if (card.status === expected.status)
    return { kind: "permission-terminal", card };
  if (TERMINAL_PERMISSION_STATUSES.includes(card.status))
    return { kind: "permission-mismatch", cards };
  return null;
}

export function resolvePermissionExpression(
  preamble,
  turnId,
  decision,
  identity,
) {
  const expected = permissionDecision(decision);
  if (!expected) throw new Error("hr6-permission-decision-unsupported");
  const expectation = permissionExpectation(identity?.expected);
  const wanted = permissionIdentity(identity);
  if (
    wanted.itemId === null &&
    (wanted.label === null || wanted.detail === null)
  )
    throw new Error("hr6-permission-identity-invalid");
  return `(()=>{${preamble}
    ${turnScopeExpression(
      turnId,
      'return {outcome:"turn-lost"};',
      'return {outcome:"turn-ambiguous"};',
      'return {outcome:"turn-scan-exceeded"};',
    )}
    const find=${boundedMatchProjection(PERMISSION_LIMIT)};
    const name=${boundedNodeTextProjection(ACTION_TEXT_LIMIT)};
    const label=${boundedNodeTextProjection(TERMINAL_LIMITS.label)};
    const detail=${boundedNodeTextProjection()};
    const scalar=${boundedScalarProjection()};
    const scan=find(target,".acp-permission",false);
    if(scan.exceeded)return {outcome:"permission-scan-exceeded"};
    if(scan.nodes.length===0)return {outcome:"permission-absent"};
    if(scan.nodes.length>1)return {outcome:"permission-ambiguous"};
    const card=scan.nodes[0];
    const itemId=scalar(card.getAttribute("data-acp-item-id"));
    if(itemId!==${JSON.stringify(wanted.itemId)})return {outcome:"permission-identity-mismatch"};
    const labelText=label(find(card,"strong",false).nodes[0]??null).text||null;
    ${summaryScanSource("card")}
    if(summaryExceeded||summaryCount!==1)return {outcome:"permission-unreadable"};
    if(itemId===null&&(labelText!==${JSON.stringify(wanted.label)}||detailText!==${JSON.stringify(wanted.detail)}))return {outcome:"permission-identity-mismatch"};
    if(!${matchExpression(expectation, "labelText", "detailText", "detailTruncated")})return {outcome:"permission-unexpected"};
    const status=find(card,'[role="status"]',false);
    if(status.exceeded!==false||status.nodes.length!==1)return {outcome:"permission-unreadable"};
    if(scalar(status.nodes[0].getAttribute("aria-label"))!==${JSON.stringify(AWAITING_PERMISSION)})return {outcome:"permission-absent"};
    const actions=find(card,".acp-permission__action",false);
    if(actions.exceeded)return {outcome:"action-scan-exceeded"};
    const chosen=[];
    for(let index=0;index<actions.nodes.length;index+=1){
      if(name(actions.nodes[index]).text===${JSON.stringify(expected.action)})chosen.push(actions.nodes[index]);
    }
    if(chosen.length!==1)return {outcome:"action-unresolved"};
    if(chosen[0].disabled)return {outcome:"action-unavailable"};
    chosen[0].click();
    return {outcome:"resolved",action:${JSON.stringify(expected.action)}};
  })()`;
}
