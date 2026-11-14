import { useRef, useState, type PointerEvent } from "react";
import { RotateCcw, X } from "../../ui/leanIcons";
import {
  clampDeviceSide,
  MAX_DEVICE_SIDE,
  MIN_DEVICE_SIDE,
  RESPONSIVE,
  rotateDevice,
  type BrowserDeviceSize,
} from "./browserDevice";
import { DEVICE_PRESETS, selectDevicePreset } from "./browserDevicePresets";
import "./browser-device.css";

interface ToolbarProps {
  size: BrowserDeviceSize;
  scalePercent: number | null;
  /** A reset step failed: the page may not match this size. */
  unconfirmed: boolean;
  onChange: (size: BrowserDeviceSize) => void;
  onClose: () => void;
}

export function BrowserDeviceToolbar({
  size,
  scalePercent,
  unconfirmed,
  onChange,
  onClose,
}: ToolbarProps) {
  const responsive = size.preset === RESPONSIVE;
  return (
    <div
      className="browser-device-toolbar"
      role="toolbar"
      aria-label="Device toolbar"
    >
      <label className="browser-device-toolbar__preset">
        <span>Dimensions:</span>
        <select
          value={size.preset}
          onChange={(event) =>
            onChange(selectDevicePreset(size, event.target.value))
          }
        >
          {DEVICE_PRESETS.map((preset) => (
            <option key={preset.id} value={preset.id}>
              {preset.label}
            </option>
          ))}
        </select>
      </label>
      <div className="browser-device-toolbar__size">
        <DeviceSideInput
          label="Width"
          value={size.width}
          editable={responsive}
          onCommit={(width) => onChange({ ...size, width })}
        />
        <span aria-hidden="true">×</span>
        <DeviceSideInput
          label="Height"
          value={size.height}
          editable={responsive}
          onCommit={(height) => onChange({ ...size, height })}
        />
        <button
          type="button"
          aria-label="Rotate"
          title="Rotate"
          onClick={() => onChange(rotateDevice(size))}
        >
          <RotateCcw size={14} aria-hidden="true" />
        </button>
        {unconfirmed ? (
          <span
            className="browser-device-toolbar__scale"
            role="status"
            title="A reset failed, so the page may not match this size"
          >
            Unconfirmed
          </span>
        ) : (
          <span
            className="browser-device-toolbar__scale"
            title="Scaled to fit the pane"
          >
            {scalePercent === null ? "–" : `${scalePercent}%`}
          </span>
        )}
      </div>
      <button
        type="button"
        className="browser-device-toolbar__close"
        aria-label="Close device toolbar"
        title="Close device toolbar"
        onClick={onClose}
      >
        <X size={14} aria-hidden="true" />
      </button>
    </div>
  );
}

function DeviceSideInput({
  label,
  value,
  editable,
  onCommit,
}: {
  label: string;
  value: number;
  editable: boolean;
  onCommit: (value: number) => void;
}) {
  const [draft, setDraft] = useState<string | null>(null);
  const commit = () => {
    if (draft === null) return;
    setDraft(null);
    const parsed = Number.parseInt(draft, 10);
    if (Number.isFinite(parsed) && clampDeviceSide(parsed) !== value)
      onCommit(clampDeviceSide(parsed));
  };
  return (
    <input
      aria-label={label}
      type="number"
      inputMode="numeric"
      min={MIN_DEVICE_SIDE}
      max={MAX_DEVICE_SIDE}
      readOnly={!editable}
      value={draft ?? String(value)}
      onChange={(event) => setDraft(event.target.value)}
      onBlur={commit}
      onKeyDown={(event) => {
        if (event.key === "Enter") commit();
        if (event.key === "Escape" && draft !== null) {
          event.stopPropagation();
          setDraft(null);
        }
      }}
    />
  );
}

type Edge = "left" | "right" | "bottom" | "bottom-left" | "bottom-right";
const EDGES: readonly Edge[] = [
  "left",
  "right",
  "bottom",
  "bottom-left",
  "bottom-right",
];

/**
 * The device stays centred, so a side handle changes the width by twice its
 * travel.
 */
export function BrowserDeviceHandles({
  size,
  scalePercent,
  onChange,
}: {
  size: BrowserDeviceSize;
  scalePercent: number;
  onChange: (size: BrowserDeviceSize) => void;
}) {
  const drag = useRef<{
    edge: Edge;
    x: number;
    y: number;
    size: BrowserDeviceSize;
    scale: number;
  } | null>(null);
  if (size.preset !== RESPONSIVE) return null;
  const start = (edge: Edge, event: PointerEvent<HTMLDivElement>) => {
    event.preventDefault();
    event.currentTarget.setPointerCapture(event.pointerId);
    drag.current = {
      edge,
      x: event.clientX,
      y: event.clientY,
      size,
      scale: scalePercent / 100,
    };
  };
  const move = (event: PointerEvent<HTMLDivElement>) => {
    const current = drag.current;
    if (!current) return;
    const dx = (event.clientX - current.x) / current.scale;
    const dy = (event.clientY - current.y) / current.scale;
    const horizontal = current.edge.endsWith("left")
      ? -2 * dx
      : current.edge.endsWith("right")
        ? 2 * dx
        : 0;
    const vertical = current.edge.startsWith("bottom") ? dy : 0;
    onChange({
      preset: RESPONSIVE,
      width: clampDeviceSide(current.size.width + horizontal),
      height: clampDeviceSide(current.size.height + vertical),
    });
  };
  const end = () => {
    drag.current = null;
  };
  return EDGES.map((edge) => (
    <div
      key={edge}
      aria-hidden="true"
      className={`browser-device-handle browser-device-handle--${edge}`}
      onPointerDown={(event) => start(edge, event)}
      onPointerMove={move}
      onPointerUp={end}
      onPointerCancel={end}
    />
  ));
}
