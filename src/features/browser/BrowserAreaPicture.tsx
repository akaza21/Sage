import { useEffect, useRef, useState } from "react";
import {
  browserAnnotateCrop,
  type BrowserAnnotateCrop,
} from "../../ports/nativeBrowserAnnotatePort";

export function BrowserAreaPicture({
  tabId,
  onChange,
}: {
  tabId: string;
  onChange: (crop: BrowserAnnotateCrop | null, blocked: boolean) => void;
}) {
  const [checked, setChecked] = useState(false);
  const [crop, setCrop] = useState<BrowserAnnotateCrop | null>(null);
  const [error, setError] = useState<string | null>(null);
  const request = useRef(0);
  useEffect(
    () => () => {
      request.current += 1;
    },
    [],
  );
  const toggle = (enabled: boolean) => {
    const current = ++request.current;
    setChecked(enabled);
    setCrop(null);
    setError(null);
    onChange(null, enabled);
    if (!enabled) return;
    void browserAnnotateCrop(tabId)
      .then((next) => {
        if (request.current !== current) return;
        setCrop(next);
        onChange(next, false);
      })
      .catch((cause: unknown) => {
        if (request.current === current) setError(String(cause).slice(0, 256));
      });
  };
  return (
    <aside className="browser-annotate__picture">
      <label>
        <input
          type="checkbox"
          checked={checked}
          onChange={(event) => toggle(event.target.checked)}
        />
        Include a picture of this area
      </label>
      {checked ? (
        <>
          <p>Pictures can include secrets shown on the page</p>
          <p>Preview only; pictures are not sent yet.</p>
          {crop ? (
            <img
              src={`data:image/png;base64,${crop.pngBase64}`}
              alt="Masked area preview"
            />
          ) : (
            <p role="status">Preparing masked preview…</p>
          )}
          {error ? <p role="alert">{error}</p> : null}
          <button type="button" onClick={() => toggle(false)}>
            Remove picture
          </button>
        </>
      ) : null}
    </aside>
  );
}
