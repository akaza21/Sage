// Pi's wire contract, from the RPC protocol declarations Pi publishes in
// `@earendil-works/pi-coding-agent` (`rpc-types.ts`, "the exported
// TypeScript protocol definitions"; Pi publishes no JSON Schema for RPC).
// Commands go in on stdin; responses, `AgentSessionEvent`s and extension UI
// requests come out on stdout.
import fs from "node:fs";
import path from "node:path";

import {
  Graph,
  declarations,
  installed,
  literals,
  members,
  variants,
} from "./contract-ts.mjs";

const PI = "@earendil-works/pi-coding-agent";

function add(map, kind, ids) {
  map[kind] = [...new Set([...(map[kind] ?? []), ...ids])].sort();
}

function split(api, graph, map, root, discriminators, prefix = "") {
  for (const variant of variants(api.checker, root, discriminators)) {
    const kind = `${prefix}${variant.parts.join("/")}`;
    add(map, kind, graph.refs(variant.root, kind, variant.narrow));
  }
}

export async function contract(version, work) {
  const wanted = version.match(/\d+\.\d+\.\d+/)[0];
  const dir = installed(`${PI}@${wanted}`, work);
  const found = JSON.parse(
    fs.readFileSync(path.join(dir, "package.json"), "utf8"),
  ).version;
  if (found !== wanted)
    throw new Error(`${PI} installed ${found}, not ${wanted}`);
  const api = declarations(
    path.join(dir, "dist/modes/rpc/rpc-types.d.ts"),
    path.join(dir, "dist/core/agent-session.d.ts"),
  );
  const graph = new Graph(api.checker);
  const inbound = {};
  const outbound = {};
  // `response/<command>`; a failed response may answer any command.
  const commands = members(api.named("RpcCommand")).flatMap((command) =>
    literals(api.checker, command, "type"),
  );
  for (const response of members(api.named("RpcResponse"))) {
    const answered = literals(api.checker, response, "command");
    const kinds = answered.length ? answered : commands;
    for (const command of kinds)
      add(
        inbound,
        `response/${command}`,
        graph.refs(response, `response/${command}`),
      );
  }
  split(api, graph, inbound, api.named("AgentSessionEvent"), [
    "type",
    "assistantMessageEvent.type",
  ]);
  // Extension UI requests carry `id` and `method`, so `shape.mjs` names them
  // as requests.
  split(
    api,
    graph,
    inbound,
    api.named("RpcExtensionUIRequest"),
    ["method"],
    "request:",
  );
  split(api, graph, outbound, api.named("RpcCommand"), ["type"]);
  split(api, graph, outbound, api.named("RpcExtensionUIResponse"), ["type"]);
  return {
    version: wanted,
    source: `${PI}@${wanted} rpc-types.d.ts, agent-session.d.ts`,
    in: inbound,
    out: outbound,
    types: graph.types,
  };
}
