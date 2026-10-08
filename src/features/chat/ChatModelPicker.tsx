import { useId, useMemo, useState } from "react";
import { Check } from "../../ui/leanIcons";

export interface ComposerModelChoice {
  id: string;
  name: string;
  contextWindow?: number;
}

const FAVORITES_KEY = "sage:composer:model-favorites:v1";
const MAX_FAVORITES = 100;
const MAX_VISIBLE_MODELS = 200;

function readFavorites(): string[] {
  try {
    const stored: unknown = JSON.parse(
      localStorage.getItem(FAVORITES_KEY) ?? "[]",
    );
    return Array.isArray(stored)
      ? stored
          .filter((item): item is string => typeof item === "string")
          .slice(0, MAX_FAVORITES)
      : [];
  } catch {
    return [];
  }
}

function favoriteKey(harnessId: string, modelId: string): string {
  return `${harnessId}\u0000${modelId}`;
}

export function ChatModelPicker({
  harnessId,
  harnessName,
  models,
  selectedId,
  disabled,
  onSelect,
}: {
  harnessId: string;
  harnessName: string;
  models: readonly ComposerModelChoice[];
  selectedId: string | null;
  disabled: boolean;
  onSelect: (modelId: string) => void;
}) {
  const [query, setQuery] = useState("");
  const [activeIndex, setActiveIndex] = useState(-1);
  const [favoritesOnly, setFavoritesOnly] = useState(false);
  const [favorites, setFavorites] = useState(readFavorites);
  const gridId = useId();
  const modelCellId = (modelId: string) =>
    `${gridId}-model-${encodeURIComponent(modelId)}`;
  const visible = useMemo(() => {
    const normalized = query.trim().toLocaleLowerCase();
    return models
      .filter(
        (model) =>
          (!favoritesOnly ||
            favorites.includes(favoriteKey(harnessId, model.id))) &&
          (!normalized || model.name.toLocaleLowerCase().includes(normalized)),
      )
      .slice(0, MAX_VISIBLE_MODELS);
  }, [favorites, favoritesOnly, harnessId, models, query]);

  function toggleFavorite(modelId: string) {
    const key = favoriteKey(harnessId, modelId);
    const next = favorites.includes(key)
      ? favorites.filter((item) => item !== key)
      : [...favorites, key].slice(-MAX_FAVORITES);
    setFavorites(next);
    try {
      localStorage.setItem(FAVORITES_KEY, JSON.stringify(next));
    } catch {
      return;
    }
  }

  return (
    <>
      <div className="composer-model-picker__header">
        <button
          type="button"
          className="composer-model-picker__favorites-filter"
          aria-label="Favorites"
          aria-pressed={favoritesOnly}
          onClick={() => setFavoritesOnly((current) => !current)}
        >
          <span aria-hidden="true">★</span>
        </button>
        <input
          type="search"
          role="combobox"
          aria-label="Search models"
          aria-autocomplete="list"
          aria-haspopup="grid"
          aria-expanded="true"
          aria-controls={gridId}
          aria-activedescendant={
            visible[activeIndex]
              ? modelCellId(visible[activeIndex].id)
              : undefined
          }
          placeholder="Search models…"
          value={query}
          onChange={(event) => {
            setQuery(event.target.value);
            setActiveIndex(-1);
          }}
          onKeyDown={(event) => {
            if (event.key === "ArrowDown" || event.key === "ArrowUp") {
              event.preventDefault();
              setActiveIndex((current) =>
                Math.max(
                  0,
                  Math.min(
                    visible.length - 1,
                    current + (event.key === "ArrowDown" ? 1 : -1),
                  ),
                ),
              );
            } else if (
              event.key === "Enter" &&
              visible[activeIndex] &&
              !disabled
            ) {
              event.preventDefault();
              onSelect(visible[activeIndex].id);
            }
          }}
        />
      </div>
      <div
        id={gridId}
        className="composer-model-picker__list"
        role="grid"
        aria-label="Models"
      >
        {visible.map((model, index) => {
          const favorite = favorites.includes(favoriteKey(harnessId, model.id));
          return (
            <div
              className="composer-model-picker__row"
              role="row"
              key={model.id}
            >
              <button
                id={modelCellId(model.id)}
                type="button"
                role="gridcell"
                className="composer-model-picker__option"
                aria-label={`${model.name} — ${harnessName}`}
                aria-selected={model.id === selectedId}
                data-active={index === activeIndex || undefined}
                disabled={disabled}
                onClick={() => onSelect(model.id)}
              >
                <span>{model.name}</span>
                <small>{harnessName}</small>
                {model.id === selectedId ? (
                  <Check size={14} aria-hidden="true" />
                ) : null}
              </button>
              <div
                role="gridcell"
                aria-label="Favorite action"
                className="composer-model-picker__favorite-cell"
              >
                <button
                  type="button"
                  className="composer-model-picker__favorite"
                  aria-label={`${favorite ? "Remove favorite" : "Favorite"} ${model.name}`}
                  aria-pressed={favorite}
                  onClick={() => toggleFavorite(model.id)}
                >
                  <span aria-hidden="true">{favorite ? "★" : "☆"}</span>
                </button>
              </div>
            </div>
          );
        })}
      </div>
      {visible.length === 0 ? (
        <p className="composer-model-picker__empty">No matching models</p>
      ) : null}
      {models.length > MAX_VISIBLE_MODELS && !query ? (
        <p className="composer-model-picker__empty">
          Search to refine more models
        </p>
      ) : null}
    </>
  );
}
