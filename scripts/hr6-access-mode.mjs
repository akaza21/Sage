import {
  TERMINAL_LIMITS,
  boundedMatchProjection,
  boundedNodeTextProjection,
  boundedScalarProjection,
} from "./hr6-terminal-expression.mjs";

export const ACCESS_MODE_LIMIT = 8;
export const APPROVAL_ACCESS_MODE = "ask";
export const ACCESS_MODE_TRIGGER =
  '.composer__access-label[aria-label="Access options"]';
const ACCESS_MODE_HOST = ".composer__controls";
const ACCESS_MODE_REASON = '.composer-access-error[role="alert"]';
const LABELS = {
  readOnly: "Read-only",
  ask: "Ask each time",
  workspaceWrite: "Workspace edits",
};

export function accessModeLabels() {
  return { ...LABELS };
}

export function accessMode(id) {
  if (
    typeof id !== "string" ||
    !Object.prototype.hasOwnProperty.call(LABELS, id)
  )
    throw new Error("hr6-access-mode-unsupported");
  return { id, label: LABELS[id] };
}

function triggerScan() {
  return `const find=${boundedMatchProjection(ACCESS_MODE_LIMIT)};
    const scan=find(surface,${JSON.stringify(ACCESS_MODE_TRIGGER)},false);`;
}

export function accessModeStateExpression(preamble) {
  return `(()=>{${preamble}
    ${triggerScan()}
    const walk=${boundedNodeTextProjection(TERMINAL_LIMITS.label)};
    const scalar=${boundedScalarProjection()};
    const out={kind:"access-mode",selectedId:null,label:null,expanded:null,disabled:null,reason:null,triggers:scan.nodes.length,triggersExceeded:scan.exceeded};
    if(scan.exceeded||scan.nodes.length!==1)return out;
    const trigger=scan.nodes[0];
    out.selectedId=scalar(trigger.getAttribute("data-selected-id"));
    out.label=walk(find(trigger,".composer__access-text",false).nodes[0]??null).text||null;
    out.expanded=trigger.getAttribute("aria-expanded")==="true";
    out.disabled=trigger.disabled===true;
    const host=trigger.closest?trigger.closest(${JSON.stringify(ACCESS_MODE_HOST)}):null;
    const reason=host?find(host,${JSON.stringify(ACCESS_MODE_REASON)},false):{nodes:[],exceeded:false};
    if(reason.exceeded||reason.nodes.length>1)out.reason="hr6-access-mode-reason-unreadable";
    else if(reason.nodes.length===1)out.reason=walk(reason.nodes[0]).text||"hr6-access-mode-reason-empty";
    return out;
  })()`;
}

export function accessModeReadiness(state, id) {
  const mode = accessMode(id);
  if (!state || state.kind !== "access-mode")
    return {
      kind: typeof state?.kind === "string" ? state.kind : "read-invalid",
    };
  if (state.triggersExceeded !== false)
    return { kind: "access-mode-scan-exceeded" };
  if (state.triggers > 1) return { kind: "access-mode-ambiguous" };
  if (state.triggers !== 1) return { kind: "access-mode-absent" };
  if (state.disabled !== false) return { kind: "access-mode-disabled" };
  if (typeof state.reason === "string")
    return { kind: "access-mode-error", reason: state.reason };
  if (state.selectedId !== mode.id || state.label !== mode.label)
    return {
      kind: "access-mode-mismatch",
      selectedId: state.selectedId,
      label: state.label,
    };
  return null;
}

export function accessModeSettled(state, id, now, until) {
  const readiness = accessModeReadiness(state, id);
  if (!Number.isFinite(now) || !Number.isFinite(until) || now >= until)
    return { kind: "access-mode-late" };
  return readiness;
}

export function openAccessModeExpression(preamble) {
  return `(()=>{${preamble}
    ${triggerScan()}
    if(scan.exceeded||scan.nodes.length!==1)return false;
    const trigger=scan.nodes[0];
    if(trigger.disabled===true)return false;
    if(trigger.getAttribute("aria-expanded")!=="true")trigger.click();
    return true;
  })()`;
}

export function accessModeExpandedExpression(preamble) {
  return `(()=>{${preamble}
    ${triggerScan()}
    if(scan.exceeded||scan.nodes.length!==1)return false;
    return scan.nodes[0].getAttribute("aria-expanded")==="true";
  })()`;
}

export function chooseAccessModeExpression(preamble, id) {
  const mode = accessMode(id);
  return `(()=>{${preamble}
    ${triggerScan()}
    if(scan.exceeded)return {outcome:"access-mode-scan-exceeded"};
    if(scan.nodes.length>1)return {outcome:"access-mode-ambiguous"};
    if(scan.nodes.length!==1)return {outcome:"access-mode-absent"};
    const walk=${boundedNodeTextProjection(TERMINAL_LIMITS.label)};
    const trigger=scan.nodes[0];
    const listId=trigger.getAttribute("aria-controls");
    const list=listId?document.getElementById(listId):null;
    if(!list)return {outcome:"picker-closed"};
    const options=find(list,'[role="menuitemradio"]',false);
    if(options.exceeded)return {outcome:"option-scan-exceeded"};
    const matched=[];
    for(let index=0;index<options.nodes.length;index+=1){
      if(options.nodes[index].getAttribute("data-option-id")===${JSON.stringify(mode.id)})matched.push(options.nodes[index]);
    }
    if(matched.length===0)return {outcome:"option-absent"};
    if(matched.length>1)return {outcome:"option-ambiguous",matches:matched.length};
    const option=matched[0];
    const label=walk(find(option,"span",false).nodes[0]??null).text||null;
    if(label!==${JSON.stringify(mode.label)})return {outcome:"option-unexpected",label};
    if(option.getAttribute("aria-disabled")==="true"||option.disabled===true){
      const reason=find(option,".searchable-picker__reason",false);
      return {outcome:"option-unavailable",reason:reason.exceeded||reason.nodes.length!==1?null:walk(reason.nodes[0]).text||null};
    }
    option.click();
    return {outcome:"selected",id:${JSON.stringify(mode.id)},label:${JSON.stringify(mode.label)}};
  })()`;
}
