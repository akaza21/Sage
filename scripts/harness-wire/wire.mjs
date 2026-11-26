// A recorded newline-JSON conversation with one harness process. Every line
// either way is kept as { direction, ms, step?, frame }. `step` marks the
// client frame that starts a Sage command, so a replay knows which command to
// issue; frames without one are follow-ups of the last command.
import { spawn } from "node:child_process";

export const MAX_FRAMES = 400;

export class Wire {
  constructor({ command, args, cwd, env, shell = false }) {
    this.records = [];
    this.started = Date.now();
    this.firstFrameMs = null;
    this.queue = [];
    this.waiters = [];
    this.stderr = "";
    this.exit = null;
    this.child = spawn(command, args, {
      cwd,
      env,
      shell,
      stdio: ["pipe", "pipe", "pipe"],
      windowsHide: true,
    });
    let buffered = "";
    this.child.stdout.setEncoding("utf8");
    this.child.stdout.on("data", (chunk) => {
      buffered += chunk;
      let newline;
      while ((newline = buffered.indexOf("\n")) >= 0) {
        const line = buffered.slice(0, newline).trim();
        buffered = buffered.slice(newline + 1);
        if (line) this.receive(line);
      }
    });
    this.child.stderr.setEncoding("utf8");
    this.child.stderr.on("data", (chunk) => {
      this.stderr = (this.stderr + chunk).slice(-4000);
    });
    this.child.on("exit", (code, signal) => {
      this.exit = { code, signal };
      this.wake();
    });
    this.child.on("error", (error) => {
      this.exit = { code: null, signal: String(error.message) };
      this.wake();
    });
  }

  ms() {
    return Date.now() - this.started;
  }

  receive(line) {
    let frame;
    try {
      frame = JSON.parse(line);
    } catch {
      // A harness that prints a non-JSON line is itself drift worth keeping.
      frame = { $nonJson: line.slice(0, 2000) };
    }
    if (this.firstFrameMs === null) this.firstFrameMs = this.ms();
    this.push({ direction: "in", ms: this.ms(), frame });
    this.queue.push(frame);
    this.wake();
  }

  push(record) {
    if (this.records.length >= MAX_FRAMES) {
      throw new Error(`scenario exceeded ${MAX_FRAMES} frames`);
    }
    this.records.push(record);
  }

  send(frame, step) {
    const record = { direction: "out", ms: this.ms(), frame };
    if (step) record.step = step;
    this.push(record);
    this.child.stdin.write(`${JSON.stringify(frame)}\n`);
  }

  wake() {
    for (const waiter of this.waiters.splice(0)) waiter();
  }

  // Takes inbound frames until `done(frame)` is true. `serve(frame)` answers
  // harness requests along the way. Times out loudly rather than hanging.
  async until(done, { serve = () => {}, timeoutMs = 240000 } = {}) {
    const deadline = Date.now() + timeoutMs;
    for (;;) {
      while (this.queue.length) {
        const frame = this.queue.shift();
        serve(frame);
        if (done(frame)) return frame;
      }
      if (this.exit) {
        throw new Error(`harness exited ${JSON.stringify(this.exit)} before the wait ended`);
      }
      const left = deadline - Date.now();
      if (left <= 0) throw new Error(`timed out after ${timeoutMs}ms`);
      await new Promise((resolve) => {
        const timer = setTimeout(resolve, Math.min(left, 1000));
        this.waiters.push(() => {
          clearTimeout(timer);
          resolve();
        });
      });
    }
  }

  // Lets trailing frames arrive, then ends the process.
  async close(settleMs = 1500) {
    await new Promise((resolve) => setTimeout(resolve, settleMs));
    while (this.queue.length) this.queue.shift();
    try {
      this.child.stdin.end();
    } catch {}
    const exited = new Promise((resolve) => {
      if (this.exit) resolve();
      else this.child.once("exit", resolve);
    });
    await Promise.race([exited, new Promise((resolve) => setTimeout(resolve, 5000))]);
    if (!this.exit) {
      if (process.platform === "win32") {
        spawn("taskkill", ["/pid", String(this.child.pid), "/t", "/f"], { windowsHide: true });
      } else {
        this.child.kill("SIGKILL");
      }
    }
  }
}

// A 96x64 PNG, left half red and right half blue, as base64.
export async function twoColourPng() {
  const zlib = await import("node:zlib");
  const w = 96;
  const h = 64;
  const raw = Buffer.alloc((w * 3 + 1) * h);
  for (let y = 0; y < h; y++) {
    for (let x = 0; x < w; x++) {
      const o = y * (w * 3 + 1) + 1 + x * 3;
      if (x < w / 2) raw[o] = 220;
      else raw[o + 2] = 220;
    }
  }
  const table = Array.from({ length: 256 }, (_, n) => {
    let c = n;
    for (let k = 0; k < 8; k++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1;
    return c >>> 0;
  });
  const crc = (buf) => {
    let c = 0xffffffff;
    for (const b of buf) c = table[(c ^ b) & 0xff] ^ (c >>> 8);
    return (c ^ 0xffffffff) >>> 0;
  };
  const chunk = (type, data) => {
    const len = Buffer.alloc(4);
    len.writeUInt32BE(data.length);
    const body = Buffer.concat([Buffer.from(type), data]);
    const sum = Buffer.alloc(4);
    sum.writeUInt32BE(crc(body));
    return Buffer.concat([len, body, sum]);
  };
  const ihdr = Buffer.alloc(13);
  ihdr.writeUInt32BE(w, 0);
  ihdr.writeUInt32BE(h, 4);
  ihdr[8] = 8;
  ihdr[9] = 2;
  return Buffer.concat([
    Buffer.from([137, 80, 78, 71, 13, 10, 26, 10]),
    chunk("IHDR", ihdr),
    chunk("IDAT", zlib.deflateSync(raw)),
    chunk("IEND", Buffer.alloc(0)),
  ]).toString("base64");
}
