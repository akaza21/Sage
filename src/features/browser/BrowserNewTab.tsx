import { BrowserFavicon, type BrowserSuggestedSites } from "./browserAddress";
import "./BrowserNewTab.css";

type NewTabProps = {
  suggestions: BrowserSuggestedSites;
  onNavigate: (url: string) => void;
  onAddress: () => void;
  onNewTab: () => void;
};

export function BrowserNewTab({
  suggestions,
  onNavigate,
  onAddress,
  onNewTab,
}: NewTabProps) {
  return (
    <div className="browser-new-tab">
      <section aria-labelledby="browser-new-tab-tools">
        <h2 id="browser-new-tab-tools">Tools</h2>
        <div className="browser-new-tab__tools">
          <button type="button" onClick={onAddress}>
            Search the web
          </button>
          <button type="button" onClick={onNewTab}>
            New tab
          </button>
        </div>
      </section>
      <section aria-labelledby="browser-new-tab-suggested">
        <h2 id="browser-new-tab-suggested">Suggested</h2>
        <div className="browser-new-tab__suggestions">
          {suggestions.map(({ url, title, favicon }) => (
            <button key={url} type="button" onClick={() => onNavigate(url)}>
              <span aria-hidden="true">
                <BrowserFavicon
                  key={favicon ?? ""}
                  value={favicon}
                  fallback="◎"
                />
              </span>
              {title?.trim() || new URL(url).host}
            </button>
          ))}
        </div>
      </section>
    </div>
  );
}
