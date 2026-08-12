import { TextDecoder } from "node:util";

const MAX_PROTOCOL_LINE_BYTES = 8 * 1_024;
const MAX_PROTOCOL_FRAME_BYTES = 4 * 1_024;
const MAX_RENDER_CHARS = 4 * MAX_PROTOCOL_FRAME_BYTES;

function decodeBase64(value, maximumBytes) {
  if (
    typeof value !== "string" ||
    value.length % 4 !== 0 ||
    !/^(?:[A-Za-z0-9+/]{4})*(?:[A-Za-z0-9+/]{2}==|[A-Za-z0-9+/]{3}=)?$/u.test(
      value,
    )
  ) {
    throw new Error("invalid captured output");
  }
  const bytes = Buffer.from(value, "base64");
  if (bytes.length > maximumBytes || bytes.toString("base64") !== value) {
    throw new Error("captured output exceeded its bound");
  }
  return bytes;
}

function boundedProtocolBytes(command) {
  const framesPerStream =
    Math.ceil(command.maximumOutputBytes / MAX_PROTOCOL_FRAME_BYTES) + 1;
  return (
    2 * Math.ceil((command.maximumOutputBytes * 4) / 3) +
    2 * framesPerStream * 512 +
    16 * 1_024
  );
}

function outputState(stream, label, maximumLineBytes, maximumOutputBytes) {
  return {
    atLineStart: true,
    decoder: new TextDecoder("utf-8"),
    label,
    lineBytes: 0,
    maximumLineBytes,
    maximumOutputBytes,
    outputBytes: 0,
    stream,
    truncated: false,
  };
}

async function writeBounded(stream, value) {
  if (!value) return;
  await new Promise((resolve, reject) => {
    let settled = false;
    let waitingForDrain = false;
    let writeComplete = false;
    const onError = (error) => {
      fail(error);
    };
    const fail = (error) => {
      if (settled) return;
      settled = true;
      stream.removeListener("error", onError);
      reject(error);
    };
    const settle = () => {
      if (!settled && writeComplete && !waitingForDrain) {
        settled = true;
        stream.removeListener("error", onError);
        resolve();
      }
    };
    waitingForDrain = !stream.write(value, (error) => {
      if (error) fail(error);
      else {
        writeComplete = true;
        settle();
      }
    });
    if (waitingForDrain) {
      stream.once("error", onError);
      stream.once("drain", () => {
        waitingForDrain = false;
        settle();
      });
    }
  });
}

async function deliverDecoded(state, text) {
  if (!text) return;
  let offset = 0;
  while (offset < text.length) {
    const newline = text.indexOf("\n", offset);
    const boundedEnd = Math.min(text.length, offset + MAX_RENDER_CHARS);
    const end = newline < 0 || newline >= boundedEnd ? boundedEnd : newline + 1;
    const piece = text.slice(offset, end);
    await writeBounded(
      state.stream,
      `${state.atLineStart ? `[${state.label}] ` : ""}${piece}`,
    );
    state.atLineStart = piece.endsWith("\n");
    offset = end;
  }
}

async function deliverData(state, bytes) {
  if (state.truncated) throw new Error("data followed truncation");
  state.outputBytes += bytes.length;
  if (state.outputBytes > state.maximumOutputBytes) {
    throw new Error("captured output exceeded its bound");
  }
  for (const value of bytes) {
    state.lineBytes = value === 10 ? 0 : state.lineBytes + 1;
    if (state.lineBytes > state.maximumLineBytes) {
      throw new Error("captured line exceeded its bound");
    }
  }
  await deliverDecoded(state, state.decoder.decode(bytes, { stream: true }));
}

async function deliverTruncation(state, kind) {
  if (state.truncated) throw new Error("duplicate truncation frame");
  if (
    (kind === "line" && state.lineBytes !== state.maximumLineBytes) ||
    (kind === "total" && state.outputBytes !== state.maximumOutputBytes)
  ) {
    throw new Error("truncation did not follow its bound");
  }
  state.truncated = true;
  await deliverDecoded(state, "…[truncated]\n");
  state.lineBytes = 0;
}

function validatedProof(result, requestNonce, sequence) {
  const reasons = [
    null,
    "command-timeout",
    "command-aborted",
    "output-overflow",
    "tree-repopulated",
  ];
  if (
    result.type !== "proof" ||
    result.schemaVersion !== 1 ||
    result.requestNonce !== requestNonce ||
    result.sequence !== sequence ||
    typeof result.settled !== "boolean" ||
    result.treeBoundaryVerified !== result.settled ||
    !Number.isSafeInteger(result.activeProcesses) ||
    (result.settled && result.activeProcesses !== 0) ||
    !Number.isSafeInteger(result.exitCode) ||
    !reasons.includes(result.reason)
  ) {
    throw new Error("invalid Job proof response");
  }
  return result;
}

export async function consumeWindowsCargoJobFrames(
  readable,
  command,
  requestNonce,
  stdout,
  stderr,
) {
  const streams = {
    stdout: outputState(
      stdout,
      command.label,
      command.maximumLineBytes,
      command.maximumOutputBytes,
    ),
    stderr: outputState(
      stderr,
      command.label,
      command.maximumLineBytes,
      command.maximumOutputBytes,
    ),
  };
  const maximumProtocolBytes = boundedProtocolBytes(command);
  let buffered = Buffer.alloc(0);
  let protocolBytes = 0;
  let proof;
  let sequence = 0;
  const handleFrame = async (bytes) => {
    if (proof) throw new Error("frame followed terminal proof");
    const frame = JSON.parse(bytes.toString("utf8"));
    if (
      frame.schemaVersion !== 1 ||
      frame.requestNonce !== requestNonce ||
      frame.sequence !== sequence
    ) {
      throw new Error("invalid Job protocol frame");
    }
    if (frame.type === "proof") {
      proof = validatedProof(frame, requestNonce, sequence);
    } else if (frame.type === "data") {
      if (!Object.hasOwn(streams, frame.stream)) {
        throw new Error("invalid output stream");
      }
      const bytes_ = decodeBase64(frame.data, MAX_PROTOCOL_FRAME_BYTES);
      await deliverData(streams[frame.stream], bytes_);
    } else if (frame.type === "truncated") {
      if (
        !Object.hasOwn(streams, frame.stream) ||
        !["line", "total"].includes(frame.kind)
      ) {
        throw new Error("invalid truncation frame");
      }
      await deliverTruncation(streams[frame.stream], frame.kind);
    } else {
      throw new Error("invalid Job protocol frame type");
    }
    sequence += 1;
  };
  for await (const chunk of readable) {
    protocolBytes += chunk.length;
    if (protocolBytes > maximumProtocolBytes) {
      throw new Error("protocol bound exceeded");
    }
    buffered = Buffer.concat([buffered, chunk]);
    let newline;
    while ((newline = buffered.indexOf(10)) >= 0) {
      if (newline === 0 || newline > MAX_PROTOCOL_LINE_BYTES) {
        throw new Error("protocol line bound exceeded");
      }
      await handleFrame(buffered.subarray(0, newline));
      buffered = buffered.subarray(newline + 1);
    }
    if (buffered.length > MAX_PROTOCOL_LINE_BYTES) {
      throw new Error("protocol line bound exceeded");
    }
  }
  if (buffered.length !== 0 || !proof) {
    throw new Error("terminal Job proof unavailable");
  }
  for (const state of Object.values(streams)) {
    await deliverDecoded(state, state.decoder.decode());
  }
  return proof;
}
