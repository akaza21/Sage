import { useUiAppearanceStore } from "../../app/uiAppearanceStore";
import {
  UI_DENSITIES,
  UI_FONT_SIZES,
  uiDensityCopy,
} from "../../domain/uiAppearance";
import { SettingsRow } from "./SettingsRow";

export function AgentAppearanceSettings() {
  const appearance = useUiAppearanceStore((state) => state.appearance);
  const ready = useUiAppearanceStore((state) => state.ready);
  const error = useUiAppearanceStore((state) => state.error);
  const setAppearance = useUiAppearanceStore((state) => state.setAppearance);
  const restore = useUiAppearanceStore((state) => state.restore);
  return (
    <>
      <p>
        Interface appearance is shared by both windows. Code font and editing
        settings remain in Editor settings.
      </p>
      <div className="agent-settings__card">
        <SettingsRow
          title="Interface font size"
          description="Adjust interface and chat text, not the code editor."
        >
          <select
            className="select"
            aria-label="Interface font size"
            disabled={!ready}
            value={appearance.fontSize}
            onChange={(event) => {
              const fontSize = UI_FONT_SIZES.find(
                (size) => size === Number(event.target.value),
              );
              if (fontSize) setAppearance({ ...appearance, fontSize });
            }}
          >
            {UI_FONT_SIZES.map((size) => (
              <option key={size} value={size}>
                {size}px
              </option>
            ))}
          </select>
        </SettingsRow>
        <SettingsRow
          title="Interface density"
          description={uiDensityCopy(appearance.density).description}
        >
          <select
            className="select"
            aria-label="Interface density"
            disabled={!ready}
            value={appearance.density}
            onChange={(event) => {
              const density = UI_DENSITIES.find(
                (item) => item === event.target.value,
              );
              if (density) setAppearance({ ...appearance, density });
            }}
          >
            {UI_DENSITIES.map((density) => (
              <option key={density} value={density}>
                {uiDensityCopy(density).label}
              </option>
            ))}
          </select>
        </SettingsRow>
      </div>
      {!ready && !error ? (
        <p role="status">Loading appearance settings...</p>
      ) : null}
      {error ? (
        <div role="alert">
          {error}
          <button
            type="button"
            className="button button--compact"
            onClick={() => void restore()}
          >
            Reload appearance
          </button>
        </div>
      ) : null}
    </>
  );
}
