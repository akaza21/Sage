import { useState } from "react";
import type { BrowserElementContext } from "../../domain/attachedContext";
import {
  browserCropPreview,
  forgetBrowserCrop,
} from "../../app/browserCropPreview";

export function BrowserCropThumbnail({
  context,
}: {
  context: BrowserElementContext;
}) {
  const [, redraw] = useState(0);
  const crop = browserCropPreview(context);
  if (!crop) return null;
  return (
    <div className="browser-element-card__picture">
      <img
        src={`data:image/png;base64,${crop.pngBase64}`}
        alt="Masked area preview"
      />
      <p>Preview only; pictures are not sent yet.</p>
      <button
        type="button"
        aria-label={`Remove picture from Area ${context.ordinal}`}
        onClick={() => {
          forgetBrowserCrop(context);
          redraw((value) => value + 1);
        }}
      >
        Remove picture
      </button>
    </div>
  );
}
