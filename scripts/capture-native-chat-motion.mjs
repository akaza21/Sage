// Motion frame bursts for the parity motion states, after the codex-live
// recorder (tools/rec.ps1): a short burst at known intervals. "wall" frames
// are taken on a wall-clock schedule (at least `intervalMs` apart; the real
// offset is recorded). "seek" frames pause the named CSS animations in the
// focused pane and seek them to i * intervalMs, so a transient fade or
// stagger is sampled at exact animation times independent of CDP latency.
import { waitForState } from "./capture-native-chat-contract.mjs";
import { clickExpression, scopeToTurn } from "./capture-native-chat-dom.mjs";

export const MAX_BURST_FRAMES = 48;
const ANIMATION_NAME = /^[a-z][a-z0-9-]{0,63}$/u;

export function validateBurstSpec(spec) {
  if (
    !spec ||
    !["wall", "seek"].includes(spec.mode) ||
    !Number.isSafeInteger(spec.frames) ||
    spec.frames < 2 ||
    spec.frames > MAX_BURST_FRAMES ||
    !Number.isSafeInteger(spec.intervalMs) ||
    spec.intervalMs < 16 ||
    spec.intervalMs > 1000 ||
    (spec.mode === "seek" &&
      (!Array.isArray(spec.animations) ||
        spec.animations.length < 1 ||
        spec.animations.length > 4 ||
        !spec.animations.every((name) => ANIMATION_NAME.test(name)))) ||
    (spec.trigger !== undefined && typeof spec.trigger !== "string")
  )
    throw new Error("burst-spec");
  return spec;
}

/**
 * Pauses (and with `atMs`, seeks) the named animations whose target is in
 * the focused pane, then waits two frames; resolves to how many it holds.
 */
export function seekExpression(names, atMs = null) {
  if (!names.every((name) => ANIMATION_NAME.test(name)))
    throw new Error("burst-spec");
  if (atMs !== null && !(Number.isSafeInteger(atMs) && atMs >= 0))
    throw new Error("burst-spec");
  return `(async()=>{const names=${JSON.stringify(names)};const held=document.getAnimations().filter(animation=>names.includes(animation.animationName) && animation.effect?.target?.closest?.('.agent-chat-pane--focused'));if(held.length>512)throw Error('probe-animation-bound');for(const animation of held){animation.pause();${atMs === null ? "" : `animation.currentTime=${atMs};`}}await new Promise(resolve=>requestAnimationFrame(()=>requestAnimationFrame(resolve)));return held.length})()`;
}

/** Takes the burst; frames carry their offsets and PNG bytes. */
export async function captureBurst({
  cdp,
  evaluate,
  spec,
  turnId,
  deadline,
  record,
  validate = () => {},
}) {
  validateBurstSpec(spec);
  const shoot = async () => {
    const png = Buffer.from(
      await cdp.screenshot(Math.min(deadline, Date.now() + 10000)),
      "base64",
    );
    validate(png);
    return png;
  };
  record("burst-start");
  if (spec.trigger) {
    const selector = scopeToTurn(spec.trigger, turnId);
    await waitForState(() => evaluate(clickExpression(selector)), deadline);
  }
  const frames = [];
  const started = Date.now();
  if (spec.mode === "seek") {
    await waitForState(
      async () => (await evaluate(seekExpression(spec.animations))) > 0,
      deadline,
    );
    for (let index = 0; index < spec.frames; index++) {
      const animationMs = index * spec.intervalMs;
      if (!(await evaluate(seekExpression(spec.animations, animationMs))))
        throw new Error("burst-animation-lost");
      frames.push({
        index,
        atMs: Date.now() - started,
        animationMs,
        png: await shoot(),
      });
    }
  } else
    for (let index = 0; index < spec.frames; index++) {
      const due = started + index * spec.intervalMs;
      if (due > deadline) throw new Error("state-timeout");
      await new Promise((resolve) =>
        setTimeout(resolve, Math.max(0, due - Date.now())),
      );
      const atMs = Date.now() - started;
      frames.push({ index, atMs, animationMs: null, png: await shoot() });
    }
  record("burst-finished");
  return { mode: spec.mode, intervalMs: spec.intervalMs, frames };
}
