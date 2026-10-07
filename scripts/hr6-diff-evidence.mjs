import {
  TERMINAL_LIMITS,
  boundedFirstProjection,
  boundedMatchProjection,
  boundedNodeTextProjection,
  boundedScalarProjection,
  boundedTurnsProjection,
} from "./hr6-terminal-expression.mjs";

export const FILE_CHANGE_LIMIT = 8;
export const DIFF_LINE_LIMIT = 32;
export const DIFF_TEXT_LIMIT = 512;
export const OPEN_DIFF_PREFIX = "Open diff ";
export const DIFF_SURFACE_LABEL = "Diff for ";
const QUALIFIER = " / ";
const ARTIFACT_PANE =
  'aside.agent-artifact-pane[aria-label="Agent artifacts"]:not(.agent-artifact-pane--collapsed)';
const TERMINAL_TURN_STATUSES = [
  "Completed",
  "Interrupted",
  "Cancelled",
  "Failed",
  "Delivery unknown",
];

export function changeLocation(action) {
  if (typeof action !== "string" || !action.startsWith(OPEN_DIFF_PREFIX))
    return null;
  const qualified = action.slice(OPEN_DIFF_PREFIX.length);
  const split = qualified.indexOf(QUALIFIER);
  if (split <= 0) return null;
  const relativePath = qualified.slice(split + QUALIFIER.length);
  if (relativePath.length === 0) return null;
  return { root: qualified.slice(0, split), relativePath };
}

function basename(relativePath) {
  const parts = String(relativePath ?? "").split("/");
  return parts[parts.length - 1];
}

function changeAction(expected) {
  if (
    typeof expected?.rootName !== "string" ||
    expected.rootName.length === 0 ||
    typeof expected.relativePath !== "string" ||
    expected.relativePath.length === 0
  )
    throw new Error("hr6-diff-location-invalid");
  return (
    OPEN_DIFF_PREFIX + expected.rootName + QUALIFIER + expected.relativePath
  );
}

function expectedCounts(expected) {
  if (
    !Number.isInteger(expected?.additions) ||
    !Number.isInteger(expected?.deletions)
  )
    return null;
  return {
    additions: "+" + String(expected.additions),
    deletions: "-" + String(expected.deletions),
  };
}

function sectionLabel(count) {
  return "Edited " + String(count) + (count === 1 ? " file" : " files");
}

function boundedCountProjection() {
  return `((label)=>(node)=>{
    if(!node)return null;
    const read=label(node);
    if(read.truncated===true)return null;
    const parsed=/^([+-])\\s{0,2}(\\d{1,7})$/u.exec(read.text);
    return parsed===null?null:parsed[1]+parsed[2];
  })(${boundedNodeTextProjection(TERMINAL_LIMITS.label)})`;
}

export function fileChangesProjection() {
  return `((find,label,scalar,count)=>(node)=>{
    const scan=find(node,"section.acp-file-changes",false);
    const out=[];
    for(let index=0;index<scan.nodes.length;index+=1){
      const group=scan.nodes[index];
      const totals=find(group,'.acp-file-changes__totals span',false);
      const counted=totals.exceeded===false&&totals.nodes.length===2;
      const rows=find(group,"ul li",false);
      const entries=[];
      for(let cursor=0;cursor<rows.nodes.length;cursor+=1){
        const row=rows.nodes[cursor];
        const actions=find(row,"button.acp-resource-action",false);
        const single=actions.exceeded===false&&actions.nodes.length===1;
        entries.push({
          action:single?scalar(actions.nodes[0].getAttribute("aria-label")):null,
          openable:single&&actions.nodes[0].disabled===false,
          name:label(find(row,".chat-file-chip__name",false).nodes[0]??null).text||null,
          root:label(find(row,".chat-file-chip__root",false).nodes[0]??null).text||null,
          additions:label(find(row,".tab__count--add",false).nodes[0]??null).text||null,
          deletions:label(find(row,".tab__count--del",false).nodes[0]??null).text||null,
          operation:label(find(row,".acp-change-operation",false).nodes[0]??null).text||null
        });
      }
      out.push({
        label:label(find(group,"strong",false).nodes[0]??null).text||null,
        additions:counted?count(totals.nodes[0]):null,
        deletions:counted?count(totals.nodes[1]):null,
        entries,
        entriesExceeded:rows.exceeded
      });
    }
    return {sections:out,exceeded:scan.exceeded};
  })(${boundedMatchProjection(FILE_CHANGE_LIMIT)},${boundedNodeTextProjection(TERMINAL_LIMITS.label)},${boundedScalarProjection()},${boundedCountProjection()})`;
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

export function fileChangesExpression(preamble, turnId) {
  return `(()=>{${preamble}
    ${turnScopeExpression(
      turnId,
      'return {kind:"turn-lost"};',
      'return {kind:"turn-ambiguous"};',
      'return {kind:"turn-scan-exceeded"};',
    )}
    const one=${boundedFirstProjection()};
    const scalar=${boundedScalarProjection()};
    const scan=(${fileChangesProjection()})(target);
    return {kind:"file-changes",turnStatus:scalar(one(target,"[data-turn-status]")?.getAttribute("aria-label")),sections:scan.sections,exceeded:scan.exceeded};
  })()`;
}

function readSections(state) {
  if (!state || typeof state.kind !== "string") return { kind: "read-invalid" };
  if (state.kind !== "file-changes") return { kind: state.kind };
  const sections = Array.isArray(state.sections) ? state.sections : [];
  if (state.exceeded !== false)
    return { kind: "file-change-scan-exceeded", sections };
  if (
    sections.some(
      (section) =>
        section?.entriesExceeded !== false ||
        typeof section?.additions !== "string" ||
        typeof section?.deletions !== "string",
    )
  )
    return { kind: "file-change-unreadable", sections };
  return { sections };
}

export function fileChangeOutcome(state, expected) {
  const totals = expectedCounts(expected);
  if (
    totals === null ||
    typeof expected.operation !== "string" ||
    expected.operation.length === 0 ||
    typeof expected.rootName !== "string" ||
    typeof expected.relativePath !== "string"
  )
    return { kind: "file-change-expectation-invalid" };
  const read = readSections(state);
  if (read.kind) return read;
  const sections = read.sections;
  if (sections.length > 1) return { kind: "file-change-ambiguous", sections };
  const entries = sections.length === 1 ? sections[0].entries : [];
  const matched = entries.filter(
    (candidate) =>
      changeLocation(candidate?.action)?.relativePath === expected.relativePath,
  );
  if (matched.length > 1) return { kind: "file-change-ambiguous", sections };
  if (matched.length === 0)
    return TERMINAL_TURN_STATUSES.includes(state.turnStatus)
      ? { kind: "file-change-absent", sections }
      : null;
  const entry = matched[0];
  if (entry.openable !== true) return { kind: "file-change-unopenable", entry };
  const section = sections[0];
  if (
    section.additions !== totals.additions ||
    section.deletions !== totals.deletions ||
    section.label !== sectionLabel(section.entries.length)
  )
    return { kind: "file-change-mismatch", entry };
  const location = changeLocation(entry.action);
  if (
    location.root !== expected.rootName ||
    entry.root !== expected.rootName ||
    entry.name !== basename(expected.relativePath)
  )
    return { kind: "file-change-mismatch", entry };
  if (entry.additions !== totals.additions)
    return { kind: "file-change-mismatch", entry };
  if (entry.deletions !== totals.deletions)
    return { kind: "file-change-mismatch", entry };
  if (entry.operation !== expected.operation)
    return { kind: "file-change-mismatch", entry };
  return { kind: "file-change", entry };
}

export function openDiffExpression(preamble, turnId, expected) {
  const wanted = changeAction(expected);
  return `(()=>{${preamble}
    ${turnScopeExpression(
      turnId,
      'return {outcome:"turn-lost"};',
      'return {outcome:"turn-ambiguous"};',
      'return {outcome:"turn-scan-exceeded"};',
    )}
    const find=${boundedMatchProjection(FILE_CHANGE_LIMIT)};
    const scalar=${boundedScalarProjection()};
    const scan=find(target,"button.acp-resource-action",false);
    if(scan.exceeded)return {outcome:"action-scan-exceeded"};
    const chosen=[];
    for(let index=0;index<scan.nodes.length;index+=1){
      const action=scalar(scan.nodes[index].getAttribute("aria-label"));
      if(typeof action!=="string")continue;
      if(action!==${JSON.stringify(wanted)})continue;
      chosen.push(scan.nodes[index]);
    }
    if(chosen.length===0)return {outcome:"action-absent"};
    if(chosen.length>1)return {outcome:"action-ambiguous"};
    if(chosen[0].disabled)return {outcome:"action-unavailable"};
    chosen[0].click();
    return {outcome:"opened",path:${JSON.stringify(expected.relativePath)}};
  })()`;
}

export function diffSurfaceExpression() {
  return `(()=>{
    const find=${boundedMatchProjection(DIFF_LINE_LIMIT)};
    const panes=${boundedMatchProjection(2)};
    const one=${boundedFirstProjection()};
    const label=${boundedNodeTextProjection(TERMINAL_LIMITS.label)};
    const count=${boundedCountProjection()};
    const scalar=${boundedScalarProjection(DIFF_TEXT_LIMIT)};
    const paneScan=panes(document.documentElement,${JSON.stringify(ARTIFACT_PANE)},false);
    if(paneScan.exceeded)return {kind:"pane-ambiguous"};
    if(paneScan.nodes.length===0)return {kind:"pane-absent"};
    if(paneScan.nodes.length>1)return {kind:"pane-ambiguous"};
    const pane=paneScan.nodes[0];
    const surfaces=panes(pane,"section.diff-surface",false);
    if(surfaces.exceeded)return {kind:"diff-ambiguous"};
    if(surfaces.nodes.length===0)return {kind:"diff-absent"};
    if(surfaces.nodes.length>1)return {kind:"diff-ambiguous"};
    const surface=surfaces.nodes[0];
    const addedScan=panes(pane,".agent-artifact-header__additions",false);
    const removedScan=panes(pane,".agent-artifact-header__deletions",false);
    const bounded=addedScan.exceeded===false&&removedScan.exceeded===false;
    const counted=bounded&&addedScan.nodes.length===1&&removedScan.nodes.length===1;
    const countsAbsent=bounded&&addedScan.nodes.length===0&&removedScan.nodes.length===0;
    const surfaceScan=find(surface,'.diff-header__counts span',false);
    const surfaceCounts=[];
    if(surfaceScan.exceeded===false){
      for(let index=0;index<surfaceScan.nodes.length;index+=1)surfaceCounts.push(label(surfaceScan.nodes[index]).text);
    }
    const rows=find(surface,".diff-line--added",false);
    const gone=find(surface,".diff-line--removed",false);
    const kept=find(surface,".diff-line--context",false);
    const hunkScan=find(surface,".diff-hunk",false);
    const lines=[];
    for(let index=0;index<rows.nodes.length;index+=1){
      const code=one(rows.nodes[index],'code[role="cell"]');
      lines.push(code?scalar(code.textContent):null);
    }
    return {
      kind:"diff-surface",
      path:scalar(surface.getAttribute("aria-label")),
      headerPath:label(one(surface,".diff-header__path span")).text||null,
      counts:counted?[count(addedScan.nodes[0]),count(removedScan.nodes[0])]:null,
      countsAbsent,
      surfaceCounts:surfaceScan.exceeded===false?surfaceCounts:null,
      hunks:hunkScan.nodes.length,
      hunksExceeded:hunkScan.exceeded,
      context:kept.nodes.length,
      contextExceeded:kept.exceeded,
      lines,
      removed:gone.nodes.length,
      removedExceeded:gone.exceeded,
      linesExceeded:rows.exceeded,
      editor:Boolean(one(surface,'div.diff-editor[role="table"][aria-label="Unified file diff"]')),
      loading:Boolean(one(surface,".resource-state--loading")),
      error:Boolean(one(surface,".resource-state--error")),
      truncated:Boolean(one(surface,".diff-limit"))
    };
  })()`;
}

export function diffSurfaceOutcome(state, expected) {
  const totals = expectedCounts(expected);
  if (
    totals === null ||
    typeof expected.relativePath !== "string" ||
    expected.relativePath.length === 0 ||
    !Array.isArray(expected.addedLines)
  )
    return { kind: "diff-expectation-invalid" };
  if (!state || typeof state.kind !== "string") return { kind: "read-invalid" };
  if (state.kind !== "diff-surface") return { kind: state.kind };
  if (
    state.linesExceeded !== false ||
    state.removedExceeded !== false ||
    state.hunksExceeded !== false ||
    state.contextExceeded !== false
  )
    return { kind: "diff-scan-exceeded" };
  if (state.truncated === true) return { kind: "diff-truncated" };
  if (state.error === true) return { kind: "diff-error" };
  if (state.loading === true || state.editor !== true || state.hunks === 0)
    return null;
  if (state.hunks !== 1)
    return { kind: "diff-hunks-mismatch", hunks: state.hunks };
  if (state.context !== 0)
    return { kind: "diff-context-lines", context: state.context };
  if (state.removed !== 0) return { kind: "diff-removed-lines" };
  const counts = Array.isArray(state.counts) ? state.counts : null;
  if (counts === null) {
    if (state.countsAbsent === true) return { kind: "diff-counts-pending" };
    return { kind: "diff-counts-mismatch", counts: null };
  }
  if (
    counts.length !== 2 ||
    counts[0] !== totals.additions ||
    counts[1] !== totals.deletions
  )
    return { kind: "diff-counts-mismatch", counts };
  const surfaceCounts = Array.isArray(state.surfaceCounts)
    ? state.surfaceCounts
    : null;
  if (surfaceCounts === null || surfaceCounts.length !== 2)
    return { kind: "diff-counts-unreadable", surfaceCounts };
  const wantedCounts = [totals.additions, totals.deletions];
  if (
    surfaceCounts.some(
      (entry, index) =>
        typeof entry !== "string" ||
        (entry.length > 0 && entry !== wantedCounts[index]),
    )
  )
    return { kind: "diff-counts-mismatch", counts: surfaceCounts };
  if (
    state.path !== DIFF_SURFACE_LABEL + expected.relativePath ||
    state.headerPath !== expected.relativePath
  )
    return { kind: "diff-path-mismatch" };
  const projected = Array.isArray(state.lines) ? state.lines : [];
  if (
    projected.some((line) => typeof line !== "string" || !line.startsWith("+"))
  )
    return { kind: "diff-unreadable" };
  const lines = projected.map((line) => line.slice(1));
  const wanted = Array.isArray(expected.addedLines) ? expected.addedLines : [];
  if (
    lines.length !== wanted.length ||
    lines.some((line, index) => line !== wanted[index])
  )
    return { kind: "diff-lines-mismatch", lines };
  return { kind: "diff-surface", lines };
}
