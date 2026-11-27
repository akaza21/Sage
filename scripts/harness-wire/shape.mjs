#!/usr/bin/env node
// The shape of a harness's inbound wire: every frame kind it sent, each kind's
// field paths, and the commands and skills it listed. A new harness version
// that adds a field (Claude Code 2.1.330's `is_backgrounded`), adds an event
// type or drops a command changes the shape, and the diff reports it.
//
//   node scripts/harness-wire/shape.mjs --diff <committed corpus> <new capture>
//   node scripts/harness-wire/shape.mjs --write <corpus>   (rewrite shape.json)
import { createHash } from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import { pathToFileURL } from "node:url";

export const MAX_DEPTH = 4;
// Subtrees whose keys are the model's or the user's data, not the protocol.
export const OPAQUE = new Set([
  "input",
  "updatedInput",
  "arguments",
  "args",
  "settings",
  "applied",
  "effective",
  "env",
  "properties",
  "structuredContent",
  "partialResult",
  "result_json",
]);

function text(value) {
  return typeof value === "string" ? value : undefined;
}

// A protocol-agnostic name for one frame: stream-json `type/subtype`, the
// event inside a stream event or Pi message update, a JSON-RPC method, or the
// method a JSON-RPC or Pi response answers.
export function frameKind(frame, sent) {
  if (frame.$nonJson !== undefined) return "$nonJson";
  if (text(frame.method)) {
    const item = frame.params?.item?.type;
    return [
      frame.id !== undefined ? "request" : "notify",
      frame.method,
      text(item),
    ]
      .filter(Boolean)
      .join(":");
  }
  if (frame.id !== undefined && !text(frame.type)) {
    return `${frame.error ? "error" : "result"}:${sent.get(String(frame.id)) ?? "?"}`;
  }
  const parts = [frame.type, frame.subtype];
  if (frame.type === "response")
    parts.push(text(frame.command) ?? sent.get(String(frame.id)));
  if (frame.type === "control_request") parts.push(frame.request?.subtype);
  if (frame.type === "control_response") {
    parts.push(
      frame.response?.subtype,
      sent.get(String(frame.response?.request_id)),
    );
  }
  if (frame.event)
    parts.push(
      frame.event.type,
      frame.event.delta?.type,
      frame.event.content_block?.type,
    );
  if (frame.assistantMessageEvent) parts.push(frame.assistantMessageEvent.type);
  return parts.filter((part) => typeof part === "string" && part).join("/");
}

function fields(value, prefix, depth, out) {
  if (Array.isArray(value)) {
    for (const entry of value) fields(entry, `${prefix}[]`, depth, out);
    return;
  }
  if (!value || typeof value !== "object" || depth >= MAX_DEPTH) return;
  for (const [key, entry] of Object.entries(value)) {
    const name = prefix ? `${prefix}.${key}` : key;
    out.add(name);
    if (!OPAQUE.has(key)) fields(entry, name, depth + 1, out);
  }
}

function commandNames(value, out) {
  if (Array.isArray(value))
    return value.forEach((entry) => commandNames(entry, out));
  if (!value || typeof value !== "object") return;
  for (const [key, entry] of Object.entries(value)) {
    if (
      ["commands", "slash_commands", "skills"].includes(key) &&
      Array.isArray(entry)
    ) {
      for (const command of entry) {
        const name =
          typeof command === "string" ? command : text(command?.name);
        if (name) out.add(`${key}:${name}`);
      }
    }
    if (!OPAQUE.has(key)) commandNames(entry, out);
  }
}

export function shapeOfRecords(
  records,
  shape = { kinds: {}, commands: new Set() },
) {
  const sent = new Map();
  for (const { direction, frame } of records) {
    if (direction === "out") {
      const id = frame.request_id ?? frame.id;
      const name = frame.method ?? frame.request?.subtype ?? frame.type;
      if (id !== undefined && name) sent.set(String(id), name);
      continue;
    }
    const kind = frameKind(frame, sent);
    const set = (shape.kinds[kind] ??= new Set());
    fields(frame, "", 0, set);
    commandNames(frame, shape.commands);
  }
  return shape;
}

// Records, with each `{ ref: "commands.jsonl", sha256 }` replaced by the
// frame of the line of that file whose hash starts with `sha256`.
export function readTranscript(file) {
  const lines = (name) =>
    fs
      .readFileSync(path.join(path.dirname(file), name), "utf8")
      .split("\n")
      .filter((line) => line.trim());
  return lines(path.basename(file)).map((line) => {
    const record = JSON.parse(line);
    if (!record.ref) return record;
    const stored = lines(record.ref).find((candidate) =>
      createHash("sha256")
        .update(candidate)
        .digest("hex")
        .startsWith(record.sha256),
    );
    if (!stored)
      throw new Error(
        `${file}: no line of ${record.ref} has digest ${record.sha256}`,
      );
    return { ...record, frame: JSON.parse(stored).frame };
  });
}

export function shapeOfHarness(dir) {
  const shape = { kinds: {}, commands: new Set() };
  for (const name of fs
    .readdirSync(dir)
    .filter((entry) => entry.endsWith(".jsonl"))
    .sort()) {
    shapeOfRecords(readTranscript(path.join(dir, name)), shape);
  }
  const meta = JSON.parse(fs.readFileSync(path.join(dir, "meta.json"), "utf8"));
  return {
    version: meta.version,
    commands: [...shape.commands].sort(),
    kinds: Object.fromEntries(
      Object.keys(shape.kinds)
        .sort()
        .map((kind) => [kind, [...shape.kinds[kind]].sort()]),
    ),
  };
}

// Every difference between two shapes, as report lines.
export function diffShapes(before, after) {
  const lines = [];
  if (before.version !== after.version)
    lines.push(`version: ${before.version} -> ${after.version}`);
  const old = new Set(before.commands);
  const now = new Set(after.commands);
  for (const command of old)
    if (!now.has(command)) lines.push(`removed command ${command}`);
  for (const command of now)
    if (!old.has(command)) lines.push(`added command ${command}`);
  for (const kind of Object.keys(before.kinds)) {
    if (!(kind in after.kinds)) lines.push(`removed event ${kind}`);
  }
  for (const [kind, list] of Object.entries(after.kinds)) {
    if (!(kind in before.kinds)) {
      lines.push(`added event ${kind}`);
      continue;
    }
    const was = new Set(before.kinds[kind]);
    const is = new Set(list);
    for (const field of was)
      if (!is.has(field)) lines.push(`removed field ${kind} ${field}`);
    for (const field of is)
      if (!was.has(field)) lines.push(`added field ${kind} ${field}`);
  }
  return lines;
}

// Startup that more than doubled and grew by over two seconds.
export function diffTiming(before, after) {
  const lines = [];
  for (const [scenario, now] of Object.entries(after.scenarios ?? {})) {
    const was = before.scenarios?.[scenario]?.firstFrameMs;
    if (
      typeof was === "number" &&
      typeof now.firstFrameMs === "number" &&
      now.firstFrameMs > was * 2 &&
      now.firstFrameMs - was > 2000
    ) {
      lines.push(
        `slower start ${scenario}: first frame ${was}ms -> ${now.firstFrameMs}ms`,
      );
    }
    if (
      before.scenarios?.[scenario] &&
      before.scenarios[scenario].outcome !== now.outcome
    ) {
      lines.push(
        `outcome ${scenario}: ${before.scenarios[scenario].outcome} -> ${now.outcome}`,
      );
    }
  }
  return lines;
}

function harnessDirs(root) {
  return fs
    .readdirSync(root, { withFileTypes: true })
    .filter(
      (entry) =>
        entry.isDirectory() &&
        fs.existsSync(path.join(root, entry.name, "meta.json")),
    )
    .map((entry) => entry.name)
    .sort();
}

function main() {
  const [mode, first, second] = process.argv.slice(2);
  if (mode === "--write") {
    for (const harness of harnessDirs(first)) {
      const dir = path.join(first, harness);
      fs.writeFileSync(
        path.join(dir, "shape.json"),
        `${JSON.stringify(shapeOfHarness(dir), null, 2)}\n`,
      );
    }
    return;
  }
  if (mode !== "--diff" || !second) {
    console.error(
      "usage: shape.mjs --diff <before> <after> | --write <corpus>",
    );
    process.exit(2);
  }
  let changed = 0;
  for (const harness of harnessDirs(second)) {
    const was = path.join(first, harness);
    if (!fs.existsSync(path.join(was, "meta.json"))) {
      console.log(`${harness}: no committed corpus to compare`);
      changed += 1;
      continue;
    }
    const meta = (dir) =>
      JSON.parse(fs.readFileSync(path.join(dir, "meta.json"), "utf8"));
    const lines = [
      ...diffShapes(
        shapeOfHarness(was),
        shapeOfHarness(path.join(second, harness)),
      ),
      ...diffTiming(meta(was), meta(path.join(second, harness))),
    ];
    for (const line of lines) console.log(`${harness}: ${line}`);
    changed += lines.length;
  }
  if (changed) {
    console.log(
      `${changed} drift entries; triage each before admitting the recapture.`,
    );
    process.exit(1);
  }
  console.log("no drift");
}

if (import.meta.url === pathToFileURL(process.argv[1] ?? "").href) main();
