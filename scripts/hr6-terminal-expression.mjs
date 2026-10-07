export const TERMINAL_LIMITS = {
  turns: 24,
  tools: 16,
  label: 160,
  text: 512,
  alerts: 8,
  messages: 8,
  blocks: 24,
  nodes: 2048,
  options: 16,
  resources: 32,
  identity: 128,
  marker: "[hr6-truncated]",
};

export function boundedMatchProjection(
  limit,
  nodes = TERMINAL_LIMITS.nodes,
  probe = true,
) {
  const cap = probe
    ? "if(found.length>limit){exceeded=true;found.pop();break;}"
    : "if(found.length>=limit)break;";
  return `((limit,budget)=>(root,selector,fromEnd,prune)=>{
    if(!root||!root.children)return {nodes:[],exceeded:false};
    const found=[];
    const stack=[root];
    let visited=0;
    let exceeded=false;
    while(stack.length>0){
      const node=stack.pop();
      visited+=1;
      if(visited>budget){exceeded=true;break;}
      if(node!==root&&node.matches&&node.matches(selector)){
        found.push(node);
        ${cap}
        continue;
      }
      if(prune&&node!==root&&node.matches&&node.matches(prune))continue;
      const children=node.children;
      const count=children?children.length:0;
      if(count>budget){exceeded=true;break;}
      if(fromEnd){for(let index=0;index<count;index+=1)stack.push(children[index]);}
      else{for(let index=count-1;index>=0;index-=1)stack.push(children[index]);}
    }
    return {nodes:found,exceeded};
  })(${limit},${nodes})`;
}

export function boundedFirstProjection(nodes = TERMINAL_LIMITS.nodes) {
  return `((find)=>(root,selector)=>find(root,selector,false).nodes[0]??null)(${boundedMatchProjection(1, nodes, false)})`;
}

export function boundedScopeProjection(nodes = TERMINAL_LIMITS.nodes) {
  return `((find)=>(root,selector)=>find(root,selector,true).nodes[0]??root)(${boundedMatchProjection(1, nodes, false)})`;
}

export function boundedNodeTextProjection(
  limit = TERMINAL_LIMITS.text,
  nodes = TERMINAL_LIMITS.nodes,
) {
  return `(root)=>{
    if(!root)return {text:"",truncated:false};
    const parts=[];
    const stack=[root];
    let total=0;
    let visited=0;
    let truncated=false;
    while(stack.length>0){
      const node=stack.pop();
      visited+=1;
      if(visited>${nodes}){truncated=true;break;}
      if(node.nodeType===3){
        const raw=node.nodeValue||"";
        const capped=raw.length>${limit}?raw.slice(0,${limit}):raw;
        const value=capped.trim();
        if(value.length>0){parts.push(value);total+=value.length+1;}
        if(raw.length>${limit}||total>${limit}){truncated=true;break;}
        continue;
      }
      const children=node.childNodes;
      const count=children?Math.min(children.length,${nodes}):0;
      for(let index=count-1;index>=0;index-=1)stack.push(children[index]);
    }
    const text=parts.join("\\n");
    if(text.length>${limit})return {text:text.slice(0,${limit})+${JSON.stringify(TERMINAL_LIMITS.marker)},truncated:true};
    return truncated?{text:text+${JSON.stringify(TERMINAL_LIMITS.marker)},truncated:true}:{text,truncated:false};
  }`;
}

export function boundedTailProjection() {
  return `(list,limit,project)=>{
    const size=list?list.length:0;
    const start=size>limit?size-limit:0;
    const out=[];
    for(let index=start;index<size;index+=1)out.push(project(list[index]));
    return out;
  }`;
}

export function boundedHeadProjection() {
  return `(list,limit,project)=>{
    const size=list?Math.min(list.length,limit):0;
    const out=[];
    for(let index=0;index<size;index+=1)out.push(project(list[index]));
    return out;
  }`;
}

export function boundedTurnsProjection(limit = TERMINAL_LIMITS.turns) {
  return `((find)=>(surface)=>{
    const scan=find(surface,"section.acp-turn",true);
    return {turns:scan.nodes.slice().reverse(),exceeded:scan.exceeded};
  })(${boundedMatchProjection(limit)})`;
}

export function boundedTextProjection(limit = TERMINAL_LIMITS.text) {
  return `(value)=>{const text=String(value??"");return text.length>${limit}?{text:text.slice(0,${limit})+${JSON.stringify(TERMINAL_LIMITS.marker)},truncated:true}:{text,truncated:false};}`;
}

export function boundedScalarProjection(limit = TERMINAL_LIMITS.label) {
  return `((limit)=>(value)=>{
    if(typeof value!=="string")return null;
    return value.length>limit?value.slice(0,limit)+${JSON.stringify(TERMINAL_LIMITS.marker)}:value;
  })(${limit})`;
}

export function boundedIdentityProjection(limit = TERMINAL_LIMITS.identity) {
  return `((limit)=>(value)=>typeof value==="string"&&value.length>0&&value.length<=limit?value:null)(${limit})`;
}

export function toolCardsProjection() {
  return `((walk,label,find,one,scalar,identity)=>(node)=>{
    const scan=find(node,".acp-tool-card",true);
    const cards=scan.nodes.slice().reverse();
    const out=[];
    for(let index=0;index<cards.length;index+=1){
      const card=cards[index];
      const output=one(card,'pre[aria-label="Command output"],pre[aria-label="Tool output"]');
      const bounded=walk(output);
      const resources=find(card,".acp-resource-list li",false);
      out.push({
        id:identity(card.getAttribute("data-acp-item-id")),
        state:scalar(card.getAttribute("data-activity-state")),
        kind:scalar(card.getAttribute("data-activity-kind")),
        label:label(one(card,"strong")).text||null,
        outputText:bounded.text,
        outputTruncated:bounded.truncated,
        outputVisible:Boolean(output)&&((output.getClientRects?.().length)??0)>0,
        resources:resources.nodes.length,
        resourcesExceeded:resources.exceeded,
        detail:label(one(card,".acp-details p")).text.length>0
      });
    }
    return {cards:out,exceeded:scan.exceeded};
  })(${boundedNodeTextProjection()},${boundedNodeTextProjection(TERMINAL_LIMITS.label)},${boundedMatchProjection(TERMINAL_LIMITS.tools)},${boundedFirstProjection()},${boundedScalarProjection()},${boundedIdentityProjection()})`;
}

export function toolRollupExpression(preamble, turnId = null) {
  return `(()=>{${preamble}
    const turns=${boundedTurnsProjection()};
    const find=${boundedMatchProjection(TERMINAL_LIMITS.tools)};
    const scoped=turns(surface).turns;
    let scope=${turnId === null ? "surface" : "null"};
    ${
      turnId === null
        ? ""
        : `for(let index=scoped.length-1;index>=0;index-=1){
      if(scoped[index].dataset.turnId===${JSON.stringify(turnId)}){scope=scoped[index];break;}
    }`
    }
    if(!scope)return 0;
    const found=find(scope,'details.acp-tool-rollup:not([open]),.acp-tool-card details:not([open])',false).nodes;
    for(let index=0;index<found.length;index+=1)found[index].open=true;
    return found.length;
  })()`;
}

export function toolRollupsProjection() {
  return `((find,cards,scalar)=>(node)=>{
    const scan=find(node,"details.acp-tool-rollup",false);
    const out=[];
    for(let index=0;index<scan.nodes.length;index+=1){
      const group=scan.nodes[index];
      out.push({
        kind:scalar(group.getAttribute("data-activity-kind")),
        state:scalar(group.getAttribute("data-activity-state")),
        open:group.open===true,
        cards:cards(group,".acp-tool-card",false).nodes.length
      });
    }
    return {rollups:out,exceeded:scan.exceeded};
  })(${boundedMatchProjection(TERMINAL_LIMITS.tools)},${boundedMatchProjection(TERMINAL_LIMITS.tools)},${boundedScalarProjection()})`;
}

export function assistantTextProjection() {
  return `((walk,find,one)=>(turn)=>{
    const scan=find(turn,'.acp-message[data-message-role="assistant"] .acp-markdown',false);
    const messages=scan.nodes;
    const parts=[];
    let total=0;
    let truncated=scan.exceeded!==false;
    for(let index=0;index<messages.length&&total<=${TERMINAL_LIMITS.text};index+=1){
      const markdown=messages[index];
      const blocks=markdown.children;
      const blockCount=blocks?Math.min(blocks.length,${TERMINAL_LIMITS.blocks}):0;
      if(blocks&&blocks.length>${TERMINAL_LIMITS.blocks})truncated=true;
      const readCount=blockCount>0?blockCount:1;
      for(let block=0;block<readCount&&total<=${TERMINAL_LIMITS.text};block+=1){
        const node=blockCount>0?blocks[block]:markdown;
        const code=node.classList&&node.classList.contains("acp-code-block")?one(node,"pre"):null;
        const value=walk(code??node);
        if(value.truncated)truncated=true;
        if(value.text.length>0){parts.push(value.text);total+=value.text.length+1;}
      }
    }
    if(total>${TERMINAL_LIMITS.text})truncated=true;
    const text=parts.join("\\n");
    return text.length>${TERMINAL_LIMITS.text}?{text:text.slice(0,${TERMINAL_LIMITS.text})+${JSON.stringify(TERMINAL_LIMITS.marker)},truncated:true}:{text,truncated};
  })(${boundedNodeTextProjection()},${boundedMatchProjection(TERMINAL_LIMITS.messages)},${boundedFirstProjection()})`;
}

export function terminalExpression(focusedSurfacePreamble) {
  return `(()=>{${focusedSurfacePreamble}
    const one=${boundedFirstProjection()};
    const scopeOf=${boundedScopeProjection()};
    const tools=${toolCardsProjection()};
    const rollups=${toolRollupsProjection()};
    const walk=${boundedNodeTextProjection()};
    const status=${boundedNodeTextProjection(TERMINAL_LIMITS.label)};
    const assistant=${assistantTextProjection()};
    const turns=${boundedTurnsProjection()};
    const alerts=${boundedMatchProjection(TERMINAL_LIMITS.alerts)};
    const scalar=${boundedScalarProjection()};
    const identity=${boundedIdentityProjection()};
    const composerScope=scopeOf(surface,".composer");
    const composer=one(composerScope,'textarea[aria-label="Message agent"]');
    const cancel=one(composerScope,'button[aria-label="Cancel turn"]');
    const scan=turns(surface);
    const projected=[];
    for(let index=0;index<scan.turns.length;index+=1){
      const node=scan.turns[index];
      const text=assistant(node);
      const scanned=tools(node);
      const grouped=rollups(node);
      projected.push({
        id:identity(node.dataset.turnId),
        status:scalar(one(node,"[data-turn-status]")?.getAttribute("aria-label"))||null,
        reason:scalar(one(node,"[data-turn-reason]")?.getAttribute("data-turn-reason"))||null,
        assistantText:text.text,
        assistantTextTruncated:text.truncated,
        tools:scanned.cards,
        toolsExceeded:scanned.exceeded,
        rollups:grouped.rollups,
        rollupsExceeded:grouped.exceeded
      });
    }
    const alertScan=alerts(surface,'[role="alert"]',false);
    const alerted=alertScan.nodes;
    const alertText=[];
    for(let index=0;index<alerted.length;index+=1)alertText.push(walk(alerted[index]).text);
    return {composerDisabled:composer?.disabled??null,cancelVisible:Boolean(cancel),turnCount:scan.exceeded?null:scan.turns.length,turnsExceeded:scan.exceeded,turns:projected,alerts:alertText,alertsExceeded:alertScan.exceeded,status:status(one(surface,".session-state")).text||null};
  })()`;
}
