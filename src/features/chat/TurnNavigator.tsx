import {
  useEffect,
  useId,
  useMemo,
  useRef,
  useState,
  type CSSProperties,
  type KeyboardEvent,
} from "react";
import { type TurnProjection } from "../../domain/acp";
import "./turn-navigator.css";
import {
  DASH_PITCH,
  dashWidth,
  sampleTurnIndices,
  turnPrompt,
} from "./turnNavigatorModel";
import { TurnPreviewCard } from "./TurnPreviewCard";

export const PREVIEW_DELAY_MS = 120;

const PREVIEW_HALF_HEIGHT = 49.5;

const STEP_KEYS = new Set(["ArrowUp", "ArrowDown", "Home", "End"]);

export function TurnNavigator({
  turns,
  visibleIds,
  top,
  height,
  onJump,
}: {
  turns: readonly TurnProjection[];
  visibleIds: readonly string[];
  top: number;
  height: number;
  onJump: (turnId: string) => void;
}) {
  const [hovered, setHovered] = useState<number | null>(null);
  const [focused, setFocused] = useState<number | null>(null);
  const [previewReady, setPreviewReady] = useState(false);
  const [frozenCurrent, setFrozenCurrent] = useState(0);
  const rail = useRef<HTMLOListElement>(null);
  const tooltipId = useId();
  const visible = useMemo(() => new Set(visibleIds), [visibleIds]);
  const liveCurrent = Math.max(
    0,
    turns.findIndex((turn) => visible.has(turn.id)),
  );
  const engaged = hovered !== null || focused !== null;
  // Freeze the sample while the reader is on the rail, so the dash under the
  // pointer does not change as the jump scrolls the transcript.
  const indices = useMemo(
    () =>
      sampleTurnIndices(
        turns.length,
        engaged ? frozenCurrent : liveCurrent,
        Math.floor((height || 540) / DASH_PITCH),
      ),
    [turns.length, engaged, frozenCurrent, liveCurrent, height],
  );
  const active =
    !engaged || turns.length < 2
      ? null
      : Math.min(hovered ?? focused!, indices.length - 1);
  useEffect(() => {
    if (active === null) return;
    const timer = setTimeout(() => setPreviewReady(true), PREVIEW_DELAY_MS);
    return () => clearTimeout(timer);
  }, [active]);
  if (turns.length < 2) return null;
  const rovingIndex = Math.max(
    0,
    indices.findIndex((index) => index >= liveCurrent),
  );
  const railCentre = top + height / 2;
  const railHeight = indices.length * DASH_PITCH;
  const railTop = railCentre - railHeight / 2;
  const dashCentre = railTop + (active ?? 0) * DASH_PITCH + DASH_PITCH / 2;
  const previewCentre = Math.max(
    top + PREVIEW_HALF_HEIGHT,
    Math.min(top + height - PREVIEW_HALF_HEIGHT, dashCentre),
  );
  const engage = () => {
    if (!engaged) setFrozenCurrent(liveCurrent);
  };
  const onKeyDown = (event: KeyboardEvent, position: number) => {
    if (event.key === "Escape") {
      if (previewReady) event.preventDefault();
      setPreviewReady(false);
      return;
    }
    if (!STEP_KEYS.has(event.key)) return;
    event.preventDefault();
    const next =
      event.key === "Home"
        ? 0
        : event.key === "End"
          ? indices.length - 1
          : Math.max(
              0,
              Math.min(
                indices.length - 1,
                position + (event.key === "ArrowUp" ? -1 : 1),
              ),
            );
    setHovered(null);
    setFocused(next);
    rail.current
      ?.querySelectorAll<HTMLButtonElement>("button")
      .item(next)
      ?.focus();
  };
  const activeTurn = active === null ? undefined : turns[indices[active]!];
  return (
    <div
      className="turn-navigator"
      style={{ top: railCentre }}
      onPointerLeave={() => {
        setHovered(null);
        if (focused === null) setPreviewReady(false);
      }}
    >
      <ol
        ref={rail}
        aria-label="Turn navigator"
        onBlur={(event) => {
          if (event.currentTarget.contains(event.relatedTarget)) return;
          setFocused(null);
          if (hovered === null) setPreviewReady(false);
        }}
      >
        {indices.map((index, position) => {
          const turn = turns[index]!;
          const distance = active === null ? null : position - active;
          return (
            <li key={turn.id}>
              <button
                type="button"
                tabIndex={(focused ?? rovingIndex) === position ? 0 : -1}
                aria-label={`Go to turn ${turn.ordinal}: ${turnPrompt(turn)}`}
                aria-describedby={
                  distance === 0 && previewReady ? tooltipId : undefined
                }
                aria-current={index === liveCurrent ? "location" : undefined}
                data-visible={visible.has(turn.id)}
                data-active={distance === 0}
                style={
                  {
                    "--w": `${distance === null ? 6 : dashWidth(distance)}px`,
                    "--dash-height": distance === null ? "1px" : "2px",
                  } as CSSProperties
                }
                onPointerEnter={() => {
                  engage();
                  setFocused(null);
                  setHovered(position);
                }}
                onFocus={() => {
                  if (hovered !== null) return;
                  engage();
                  setFocused(position);
                }}
                onClick={() => onJump(turn.id)}
                onKeyDown={(event) => onKeyDown(event, position)}
              />
            </li>
          );
        })}
      </ol>
      {previewReady && activeTurn ? (
        <TurnPreviewCard
          id={tooltipId}
          turn={activeTurn}
          top={previewCentre - railTop}
        />
      ) : null}
    </div>
  );
}
