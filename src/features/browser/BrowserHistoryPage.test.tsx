import {
  act,
  cleanup,
  fireEvent,
  render,
  renderHook,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import type {
  BrowserHistoryCursor,
  BrowserHistoryPage as Page,
  BrowserVisit,
} from "../../ports/nativeBrowserPort";
import { BrowserHistoryPage } from "./BrowserHistoryPage";
import { historyDayLabel } from "./historyDays";
import {
  HISTORY_MAX_LOADED,
  HISTORY_PAGE_SIZE,
  HISTORY_SEARCH_DELAY_MS,
  useBrowsingHistory,
} from "./useBrowsingHistory";

type Query = {
  search: string;
  before: BrowserHistoryCursor | null;
  limit: number;
};
const browserHistoryPage = vi.fn(async (_query: Query): Promise<Page> => ({
  visits: [],
  next: null,
}));
const browserHistoryRemove = vi.fn(
  async (_ids: readonly number[]): Promise<number> => 1,
);
vi.mock("../../ports/nativeBrowserPort", () => ({
  browserHistoryPage: (query: Query) => browserHistoryPage(query),
  browserHistoryRemove: (ids: readonly number[]) => browserHistoryRemove(ids),
}));

const NOW = new Date(2026, 9, 31, 15, 0).getTime();
const HOUR = 60 * 60 * 1000;

function visit(id: number, visitedAt: number, title: string | null = null) {
  return {
    id,
    url: `https://site${id}.example/path`,
    title,
    visitedAt,
  } satisfies BrowserVisit;
}

beforeEach(() => {
  browserHistoryPage.mockReset();
  browserHistoryPage.mockResolvedValue({ visits: [], next: null });
  browserHistoryRemove.mockReset();
  browserHistoryRemove.mockResolvedValue(1);
});

/** A visit's own button, not its Remove button. */
const opens = (text: string) => new RegExp(`^(?!Remove ).*${text}`);

afterEach(() => {
  cleanup();
  vi.useRealTimers();
});

it("lists visits newest first under day headings and opens one", async () => {
  browserHistoryPage.mockResolvedValueOnce({
    visits: [
      visit(3, NOW - HOUR, "Today page"),
      visit(2, NOW - 24 * HOUR),
      visit(1, NOW - 72 * HOUR, "Older page"),
    ],
    next: null,
  });
  const onNavigate = vi.fn();
  render(<BrowserHistoryPage onNavigate={onNavigate} onClose={vi.fn()} />);
  const page = screen.getByRole("region", { name: "History" });
  await within(page).findByRole("button", { name: opens("Today page") });
  const headings = within(page)
    .getAllByRole("heading", { level: 2 })
    .map((heading) => heading.textContent);
  expect(headings).toHaveLength(3);
  // A visit with no title shows its URL; its host is shown beside it.
  const untitled = within(page).getByRole("button", {
    name: opens("site2\\.example/path"),
  });
  expect(untitled).toHaveTextContent("site2.example");
  fireEvent.click(
    within(page).getByRole("button", { name: opens("Older page") }),
  );
  expect(onNavigate).toHaveBeenCalledWith("https://site1.example/path");
});

it("labels today and yesterday like Chrome", () => {
  expect(historyDayLabel(NOW - HOUR, NOW)).toMatch(/^Today - /);
  expect(historyDayLabel(NOW - 20 * HOUR, NOW)).toMatch(/^Yesterday - /);
  expect(historyDayLabel(NOW - 72 * HOUR, NOW)).not.toMatch(
    /^(Today|Yesterday)/,
  );
});

it("searches after typing settles and ignores a stale answer", async () => {
  vi.useFakeTimers();
  let answerFirst: ((page: Page) => void) | undefined;
  browserHistoryPage
    .mockResolvedValueOnce({ visits: [], next: null })
    .mockImplementationOnce(
      () => new Promise<Page>((resolve) => (answerFirst = resolve)),
    )
    .mockResolvedValueOnce({
      visits: [visit(9, NOW, "Rust docs")],
      next: null,
    });
  render(<BrowserHistoryPage onNavigate={vi.fn()} onClose={vi.fn()} />);
  await act(async () => undefined);
  const field = screen.getByRole("searchbox", { name: "Search history" });
  expect(field).toHaveFocus();
  fireEvent.change(field, { target: { value: "ru" } });
  await act(async () => vi.advanceTimersByTime(HISTORY_SEARCH_DELAY_MS));
  fireEvent.change(field, { target: { value: " rust " } });
  await act(async () => vi.advanceTimersByTime(HISTORY_SEARCH_DELAY_MS - 1));
  expect(browserHistoryPage).toHaveBeenCalledTimes(2);
  await act(async () => vi.advanceTimersByTime(1));
  expect(browserHistoryPage).toHaveBeenCalledTimes(3);
  expect(browserHistoryPage).toHaveBeenLastCalledWith({
    search: "rust",
    before: null,
    limit: HISTORY_PAGE_SIZE,
  });
  await act(async () => {
    answerFirst?.({ visits: [visit(8, NOW, "Stale")], next: null });
  });
  expect(screen.queryByText("Stale")).toBeNull();
  expect(screen.getByText("Rust docs")).toBeInTheDocument();
});

it("says when nothing matches and when there is no history", async () => {
  const { unmount } = render(
    <BrowserHistoryPage onNavigate={vi.fn()} onClose={vi.fn()} />,
  );
  expect(
    await screen.findByText(/No browsing history yet/),
  ).toBeInTheDocument();
  unmount();
  vi.useFakeTimers();
  render(<BrowserHistoryPage onNavigate={vi.fn()} onClose={vi.fn()} />);
  fireEvent.change(screen.getByRole("searchbox", { name: "Search history" }), {
    target: { value: "zebra" },
  });
  await act(async () => vi.advanceTimersByTime(HISTORY_SEARCH_DELAY_MS));
  expect(screen.getByRole("status")).toHaveTextContent(
    "No history matches “zebra”.",
  );
});

it("removes one visit only after Rust confirms it", async () => {
  browserHistoryPage.mockResolvedValueOnce({
    visits: [visit(2, NOW, "Keep"), visit(1, NOW - 1, "Drop")],
    next: null,
  });
  let confirm: ((count: number) => void) | undefined;
  browserHistoryRemove.mockImplementationOnce(
    () => new Promise<number>((resolve) => (confirm = resolve)),
  );
  render(<BrowserHistoryPage onNavigate={vi.fn()} onClose={vi.fn()} />);
  const remove = await screen.findByRole("button", {
    name: "Remove Drop from history",
  });
  fireEvent.click(remove);
  expect(browserHistoryRemove).toHaveBeenCalledWith([1]);
  expect(remove).toBeDisabled();
  expect(screen.getByText("Drop")).toBeInTheDocument();
  await act(async () => confirm?.(1));
  expect(screen.queryByText("Drop")).toBeNull();
  expect(screen.getByText("Keep")).toBeInTheDocument();
});

it("keeps a visit and reports the error when removal fails", async () => {
  browserHistoryPage.mockResolvedValueOnce({
    visits: [visit(1, NOW, "Stays")],
    next: null,
  });
  browserHistoryRemove.mockRejectedValueOnce("browsing history is unavailable");
  render(<BrowserHistoryPage onNavigate={vi.fn()} onClose={vi.fn()} />);
  fireEvent.click(
    await screen.findByRole("button", { name: "Remove Stays from history" }),
  );
  expect(await screen.findByRole("alert")).toHaveTextContent(
    "browsing history is unavailable",
  );
  expect(screen.getByText("Stays")).toBeInTheDocument();
});

function fullPages() {
  let start = 100_000;
  browserHistoryPage.mockImplementation(async ({ limit }) => {
    const visits = Array.from({ length: limit }, (_, index) =>
      visit(start - index, NOW - (start - index)),
    );
    start -= limit;
    return { visits, next: { visitedAt: 1, id: start } };
  });
}

it("loads an older page from the cursor when asked", async () => {
  fullPages();
  render(<BrowserHistoryPage onNavigate={vi.fn()} onClose={vi.fn()} />);
  fireEvent.click(await screen.findByRole("button", { name: "Show more" }));
  await waitFor(() =>
    expect(screen.getAllByRole("button", { name: /^Remove / })).toHaveLength(
      2 * HISTORY_PAGE_SIZE,
    ),
  );
  expect(browserHistoryPage).toHaveBeenLastCalledWith({
    search: "",
    before: { visitedAt: 1, id: 100_000 - HISTORY_PAGE_SIZE },
    limit: HISTORY_PAGE_SIZE,
  });
});

it("holds at most its bound of visits and then asks for a narrower search", async () => {
  fullPages();
  const { result } = renderHook(() => useBrowsingHistory(""));
  await waitFor(() => expect(result.current.loading).toBe(false));
  while (result.current.canLoadMore) {
    const before = result.current.visits.length;
    act(() => result.current.loadMore());
    await waitFor(() =>
      expect(result.current.visits.length).toBeGreaterThan(before),
    );
  }
  expect(result.current.visits).toHaveLength(HISTORY_MAX_LOADED);
  expect(result.current.truncated).toBe(true);
  const calls = browserHistoryPage.mock.calls.length;
  act(() => result.current.loadMore());
  expect(browserHistoryPage).toHaveBeenCalledTimes(calls);
});

it("asks only for what still fits, so no visit is skipped past the bound", async () => {
  fullPages();
  const { result } = renderHook(() => useBrowsingHistory(""));
  await waitFor(() => expect(result.current.loading).toBe(false));
  act(() => result.current.remove(result.current.visits[0]));
  await waitFor(() =>
    expect(result.current.visits).toHaveLength(HISTORY_PAGE_SIZE - 1),
  );
  while (result.current.canLoadMore) {
    const before = result.current.visits.length;
    act(() => result.current.loadMore());
    await waitFor(() =>
      expect(result.current.visits.length).toBeGreaterThan(before),
    );
  }
  expect(result.current.visits).toHaveLength(HISTORY_MAX_LOADED);
  expect(browserHistoryPage.mock.lastCall?.[0].limit).toBe(1);
});

it("never appends an older page to a list fetched again since", async () => {
  let release: (page: Page) => void = () => {};
  browserHistoryPage
    .mockResolvedValueOnce({
      visits: [visit(9, NOW)],
      next: { visitedAt: NOW, id: 9 },
    })
    .mockReturnValueOnce(
      new Promise<Page>((resolve) => {
        release = resolve;
      }),
    )
    .mockResolvedValueOnce({ visits: [visit(20, NOW)], next: null });
  const { result, rerender } = renderHook(
    ({ search }) => useBrowsingHistory(search),
    { initialProps: { search: "" } },
  );
  await waitFor(() => expect(result.current.loading).toBe(false));
  act(() => result.current.loadMore());
  // The search changes and clears before it runs: the list is fetched anew.
  rerender({ search: "x" });
  rerender({ search: "" });
  await waitFor(() =>
    expect(result.current.visits.map(({ id }) => id)).toEqual([20]),
  );
  await act(async () => release({ visits: [visit(8, NOW - 1)], next: null }));
  expect(result.current.visits.map(({ id }) => id)).toEqual([20]);
  expect(result.current.loadingMore).toBe(false);
});

it("closes on Escape and on its close button", () => {
  const onClose = vi.fn();
  render(<BrowserHistoryPage onNavigate={vi.fn()} onClose={onClose} />);
  fireEvent.keyDown(screen.getByRole("searchbox"), { key: "Escape" });
  fireEvent.click(screen.getByRole("button", { name: "Close history" }));
  expect(onClose).toHaveBeenCalledTimes(2);
});
