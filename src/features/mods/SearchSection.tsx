import {
  Fragment,
  type RefObject,
  useCallback,
  useEffect,
  useId,
  useLayoutEffect,
  useRef,
  useState,
} from 'react';
import { Button } from '../../components/Button';
import { Icon } from '../../components/Icon';
import { Notice } from '../../components/Notice';
import forms from '../../components/forms.module.css';
import { Switch } from '../../components/Switch';
import type {
  InstanceSummary,
  ModsCollection,
  ModsHit,
  ModsKind,
  ModsSearchPage,
  ModsSearchQuery,
  ModsSort,
} from '../../lib/contracts';
import { describeIpcError, ipc } from '../../lib/ipc';
import { numberLocale, strings } from '../../lib/strings';
import { daysAgo } from '../../lib/time';
import { BackToTop } from './BackToTop';
import { CatalogTile, TileSkeletons } from './CatalogTile';
import { CategoryPanel } from './CategoryPanel';
import { CollectionsStrip } from './CollectionsStrip';
import { collectionTitle } from './collections';
import { MAX_CATEGORIES, categoryName, categoryOf } from './catalogCategories';
import {
  type Committed,
  type Filters,
  type Mark,
  readMark,
  recall,
  remember,
  searchKey,
  writeMark,
} from './catalogMemory';
import { installable, loaderOf, loadersFor, validGame } from './helpers';
import styles from './Catalog.module.css';

const PAGE = 20;
/** The list stops here; a narrower search shows the rest. */
const MAX_HITS = 500;
/** How far below the screen the next page is asked for. */
const AHEAD = '600px';
const SORTS: ModsSort[] = ['relevance', 'downloads', 'trending', 'updated', 'new', 'title'];
/** Category counts the launcher reads at most. */
const MAX_COUNTS = 200;
/** The catalog's "Popular this week" stops at this many (`POPULAR` in services/mods/src/api.rs). */
const POPULAR_WEEK = 20;

/** The page of the list on screen and the search it answers. */
interface Shown extends ModsSearchPage {
  for: Committed;
  /** A later page brought nothing new (the catalog's order moved): the list ends here. */
  ended: boolean;
}

/** Where a collection's own size stops the list; its other filters bound the rest. */
function capOf(collection: ModsCollection | null): number | null {
  return collection?.preset === true && collection.id === 'popular-week' ? POPULAR_WEEK : null;
}

/** A collection that is only an order of the catalog, such as the week's popular. */
function onlyOrder(collection: ModsCollection): boolean {
  const { sort, since, source, collection: key } = collection.query;
  return sort !== null && since === null && source === null && key === null;
}

/** The search for one page of the list. */
function queryOf(kind: ModsKind, c: Committed, offset: number): ModsSearchQuery {
  const text = c.text.trim();
  const game = c.game.trim();
  const query: ModsSearchQuery = {
    text,
    kind,
    game: game === '' ? null : game,
    loader: c.loader === '' ? null : c.loader,
    category: null,
    sort: text === '' && c.sort === 'relevance' ? null : c.sort,
    offset,
    limit: PAGE,
  };
  if (c.categories.length > 0) query.categories = c.categories;
  const from = c.collection?.query;
  if (from !== undefined) {
    if (from.source !== null) query.source = from.source;
    if (from.since !== null) query.since = from.since;
    if (from.collection !== null) query.collection = from.collection;
  }
  return query;
}

/** The counts of the kind's categories the catalog sent, if they are sound. */
function countsOf(kind: ModsKind, list: unknown): Map<string, number> | null {
  if (!Array.isArray(list)) return null;
  const counts = new Map<string, number>();
  for (const item of list.slice(0, MAX_COUNTS) as unknown[]) {
    if (typeof item !== 'object' || item === null) continue;
    const { id, count } = item as Record<string, unknown>;
    if (typeof id !== 'string' || categoryOf(kind, id) === null) continue;
    if (typeof count !== 'number' || !Number.isSafeInteger(count) || count < 0) continue;
    counts.set(id, count);
  }
  return counts;
}

/** Projects without the ones already in the list (pages can shift as the catalog grows). */
function merged(before: ModsHit[], found: ModsHit[]): ModsHit[] {
  const seen = new Set(before.map((hit) => hit.id));
  return [...before, ...found.filter((hit) => !seen.has(hit.id))];
}

interface Props {
  kind: ModsKind;
  instance: InstanceSummary | null;
  onOpen: (projectId: string) => void;
  onAddToCatalog: (text: string) => void;
  /** Adds a project's best version to the installation, or opens its page. */
  onQuickAdd: (hit: ModsHit) => Promise<void>;
  /** The words in the search box, for "Add to the catalog" at the top. */
  draftTextRef: RefObject<string>;
  /** The tab that labels the list. */
  tabId: string;
}

/**
 * The catalog of one kind (D-072, variant 1 "tiles"): filters, the catalog's
 * collections, tiles that load as the page scrolls, and the categories in the column
 * at the inline end. With an installation chosen, the filters start at its game
 * version and loader. The place is kept for the rest of the run.
 */
export function SearchSection({
  kind,
  instance,
  onOpen,
  onAddToCatalog,
  onQuickAdd,
  draftTextRef,
  tabId,
}: Props) {
  const t = strings.mods;
  const c = t.catalog;
  const loaders = loadersFor(kind);
  const placeKey = `${kind}:${instance?.id ?? ''}`;
  const [place] = useState(() => recall(placeKey));
  const [draft, setDraft] = useState<Filters>(
    () =>
      place?.draft ?? {
        text: '',
        game: instance?.versionId ?? '',
        loader: loaderOf(kind, instance),
        sort: 'downloads',
      },
  );
  const [committed, setCommitted] = useState<Committed>(
    () => place?.committed ?? { ...draft, categories: [], collection: null },
  );
  const [page, setPage] = useState<Shown | null>(() =>
    place?.page == null ? null : { ...place.page, for: place.committed, ended: false },
  );
  const [error, setError] = useState<string | null>(null);
  // A page after the first that did not come: the list stays, with a retry.
  const [moreFailed, setMoreFailed] = useState(false);
  // A new search's first page, or the next page of the list.
  const [pending, setPending] = useState<'first' | 'more' | null>(
    place?.page == null ? 'first' : null,
  );
  const loading = pending !== null;
  const [counts, setCounts] = useState<{ key: string; counts: Map<string, number> } | null>(null);
  const [baseTotal, setBaseTotal] = useState<{ key: string; total: number } | null>(null);
  const [mark] = useState<(Mark & { when: string }) | null>(() => {
    // Not when the place brings its list back: that list is this run's.
    const found = place?.page == null ? readMark() : null;
    return found === null ? null : { ...found, when: daysAgo(found.at) };
  });
  const [adding, setAdding] = useState<string | null>(null);
  const [badGame, setBadGame] = useState(false);
  // Answers to older searches are dropped.
  const generation = useRef(0);
  const end = useRef<HTMLDivElement>(null);
  const tiles = useRef<HTMLUListElement>(null);
  const heading = useRef<HTMLHeadingElement>(null);
  const searchBox = useRef<HTMLInputElement>(null);
  const searchId = useId();
  const gameId = useId();
  const gameErrorId = useId();
  const loaderLabel = useId();
  const sortLabel = useId();
  const headingId = useId();

  const fits =
    instance !== null &&
    committed.game.trim() === instance.versionId &&
    committed.loader === loaderOf(kind, instance);
  // Counts and the total without categories belong to the search minus its categories.
  const baseKey = searchKey(kind, { ...committed, categories: [] });

  const fetchPage = useCallback(
    (next: Committed, offset: number) => {
      const current = ++generation.current;
      void ipc
        .modsSearch(queryOf(kind, next, offset))
        .then((found) => {
          if (current !== generation.current) return;
          const total = Math.min(found.total, capOf(next.collection) ?? found.total);
          setError(null);
          setPage((before) => {
            if (offset === 0 || before === null) {
              return { total, hits: found.hits.slice(0, total), for: next, ended: false };
            }
            const hits = merged(before.hits, found.hits).slice(0, total);
            return { total, hits, for: next, ended: hits.length === before.hits.length };
          });
          if (next.categories.length === 0 && offset === 0) {
            setBaseTotal({ key: searchKey(kind, next), total });
          }
        })
        .catch((e: unknown) => {
          if (current !== generation.current) return;
          if (offset === 0) {
            // The list on screen answers another search: only the error stays.
            setError(describeIpcError(e));
            setPage(null);
          } else {
            setMoreFailed(true);
          }
        })
        .finally(() => {
          if (current === generation.current) setPending(null);
        });
    },
    [kind],
  );

  // The first search, unless the place came back with its list.
  useEffect(() => {
    if (place?.page == null) fetchPage(committed, 0);
    // Only on open; later searches start from the controls.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [fetchPage]);

  // How many projects each category holds, for the search without its categories.
  useEffect(() => {
    if (!validGame(committed.game.trim())) return;
    let live = true;
    ipc
      .modsCategories({ ...queryOf(kind, committed, 0), categories: [], limit: null })
      .then((list) => {
        const found = countsOf(kind, list);
        if (live && found !== null) setCounts({ key: baseKey, counts: found });
      })
      .catch(() => {
        // The list shows without counts.
      });
    return () => {
      live = false;
    };
    // The counts depend on the search minus its categories, which `baseKey` names.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [baseKey]);

  const search = (next: Committed) => {
    setPending('first');
    setError(null);
    setMoreFailed(false);
    fetchPage(next, 0);
  };

  const commit = (patch: Partial<Committed>) => {
    const next: Committed = { ...committed, ...draft, ...patch };
    if (!validGame(next.game.trim())) {
      setBadGame(true);
      return;
    }
    setBadGame(false);
    setCommitted(next);
    setDraft({ text: next.text, game: next.game, loader: next.loader, sort: next.sort });
    search(next);
  };

  // The next page comes as the end of the list nears the screen.
  const more = page !== null && !page.ended && page.hits.length < Math.min(page.total, MAX_HITS);
  const loadMore = useCallback(() => {
    if (page === null || !more || pending !== null || error !== null) return;
    setPending('more');
    setMoreFailed(false);
    fetchPage(page.for, page.hits.length);
  }, [page, more, pending, error, fetchPage]);
  const watching = typeof IntersectionObserver !== 'undefined';
  useEffect(() => {
    const target = end.current;
    if (!watching || target === null || !more || loading || moreFailed) return;
    if (typeof IntersectionObserver === 'undefined') return;
    const observer = new IntersectionObserver(
      (entries) => {
        if (entries.some((entry) => entry.isIntersecting)) loadMore();
      },
      { rootMargin: `0px 0px ${AHEAD} 0px` },
    );
    observer.observe(target);
    return () => {
      observer.disconnect();
    };
  }, [watching, more, loading, moreFailed, loadMore]);

  // Back where the player left: the same list, scrolled as it was.
  useLayoutEffect(() => {
    if (place !== null && place.scrollY > 0) window.scrollTo(0, place.scrollY);
  }, [place]);

  // Kept for the rest of the run. The list only with the search it answers: a search
  // still on its way or failed is asked again on return.
  const latest = useRef({ draft, committed, page, error });
  useLayoutEffect(() => {
    latest.current = { draft, committed, page, error };
  });
  useLayoutEffect(
    () => () => {
      const { draft: d, committed: now, page: p, error: failed } = latest.current;
      const answers =
        p !== null && failed === null && searchKey(kind, p.for) === searchKey(kind, now);
      remember(placeKey, {
        draft: d,
        committed: now,
        page: answers ? { total: p.total, hits: p.hits } : null,
        scrollY: answers ? window.scrollY : 0,
      });
    },
    [placeKey, kind],
  );

  // How far down the list got, for later runs; written as it grows, since a run can
  // end with the catalog on screen.
  useEffect(() => {
    if (page === null || page.hits.length <= PAGE) return;
    writeMark({ search: searchKey(kind, page.for), count: page.hits.length, at: Date.now() });
  }, [page, kind]);

  // "Add to the catalog" at the top starts from this tab's words, and from none elsewhere.
  useLayoutEffect(() => {
    draftTextRef.current = draft.text;
    return () => {
      draftTextRef.current = '';
    };
  }, [draft.text, draftTextRef]);

  const change = (patch: Partial<Filters>) => {
    setDraft({ ...draft, ...patch });
  };

  const loaderName = (loader: string) =>
    (t.loaderNames as Record<string, string | undefined>)[loader] ?? loader;
  const game = committed.game.trim();
  const scope =
    game !== '' && committed.loader !== ''
      ? `${loaderName(committed.loader)} ${game}`
      : game !== ''
        ? game
        : committed.loader !== ''
          ? loaderName(committed.loader)
          : null;

  const toggleCategory = (id: string) => {
    const chosen = committed.categories.includes(id)
      ? committed.categories.filter((item) => item !== id)
      : [...committed.categories, id].slice(0, MAX_CATEGORIES);
    commit({ categories: chosen });
  };
  const pickCollection = (collection: ModsCollection) => {
    // Without an order of its own, a collection lists in its owner's order.
    commit({ collection, sort: collection.query.sort ?? 'relevance' });
    heading.current?.focus();
  };
  const sortBy = (sort: ModsSort) => {
    const chosen = committed.collection;
    // A collection that is only an order goes when another order is chosen.
    commit(
      chosen !== null && onlyOrder(chosen) && chosen.query.sort !== sort
        ? { sort, collection: null }
        : { sort },
    );
  };
  const reset = () => {
    commit({ categories: [], collection: null });
    heading.current?.focus();
  };
  const fitsToggle = (on: boolean) => {
    commit(
      on && instance !== null
        ? { game: instance.versionId, loader: loaderOf(kind, instance) }
        : { game: '', loader: '' },
    );
  };

  const browsing =
    committed.text.trim() === '' &&
    committed.categories.length === 0 &&
    committed.collection === null;
  const title =
    committed.collection !== null ? (
      <bdi>{collectionTitle(committed.collection)}</bdi>
    ) : committed.categories.length === 1 ? (
      categoryName(committed.categories[0] ?? '')
    ) : committed.categories.length > 1 ? (
      c.categoriesChosen({ count: committed.categories.length })
    ) : (
      c.allOf[kind]
    );
  // What the list on screen found, for the search it answers.
  const foundOf = (shown: Shown) => {
    const g = shown.for.game.trim();
    const l = shown.for.loader;
    return g !== '' && l !== ''
      ? c.foundForBoth({ count: shown.total, game: g, loader: loaderName(l) })
      : g !== ''
        ? c.foundForGame({ count: shown.total, game: g })
        : l !== ''
          ? c.foundForLoader({ count: shown.total, loader: loaderName(l) })
          : t.results({ count: shown.total });
  };
  const status = pending === 'first' ? t.loading : page === null ? '' : foundOf(page);
  const number = (value: number) => value.toLocaleString(numberLocale());
  // Where an earlier run stopped, on the same search, past its first page.
  const resumeAt =
    mark !== null && mark.search === searchKey(kind, committed) && mark.count > 0
      ? mark.count
      : null;
  const canAdd = instance !== null && kind !== 'modpack';

  return (
    <>
      {/* First in the focus order, as it is drawn: beside the tabs, or under them. */}
      <CategoryPanel
        kind={kind}
        counts={counts?.key === baseKey ? counts.counts : null}
        total={baseTotal?.key === baseKey ? baseTotal.total : null}
        selected={committed.categories}
        scope={scope}
        onToggle={toggleCategory}
        onAll={() => {
          commit({ categories: [] });
        }}
      />
      <div role="tabpanel" aria-labelledby={tabId} className={styles.panel}>
        {kind === 'modpack' && <Notice tone="info">{t.modpacksHint}</Notice>}
        <form
          className={styles.filters}
          role="search"
          onSubmit={(event) => {
            event.preventDefault();
            commit({});
          }}
        >
          <div className={[styles.field, styles.searchField].join(' ')}>
            <label htmlFor={searchId}>{t.search}</label>
            <input
              ref={searchBox}
              id={searchId}
              type="search"
              className={forms.input}
              value={draft.text}
              maxLength={200}
              placeholder={t.searchPlaceholder}
              onChange={(event) => {
                change({ text: event.target.value });
              }}
            />
          </div>
          <div className={[styles.field, styles.gameField].join(' ')}>
            <label htmlFor={gameId}>{t.gameVersion}</label>
            <input
              id={gameId}
              className={forms.input}
              value={draft.game}
              maxLength={40}
              placeholder={t.anyVersion}
              dir="ltr"
              aria-invalid={badGame || undefined}
              aria-describedby={badGame ? gameErrorId : undefined}
              onChange={(event) => {
                change({ game: event.target.value });
              }}
            />
          </div>
          <Button type="submit" disabled={pending === 'first' && page === null}>
            <Icon name="search" size={18} />
            {t.find}
          </Button>
          {badGame && (
            <p id={gameErrorId} className={styles.fieldError} role="alert">
              {c.badGame}
            </p>
          )}
        </form>
        <div className={styles.filters}>
          {loaders.length > 0 && (
            <div className={styles.pillGroup} role="group" aria-labelledby={loaderLabel}>
              <span id={loaderLabel} className={styles.pillLabel}>
                {t.loader}
              </span>
              <div className={styles.pills}>
                {['', ...loaders].map((loader) => (
                  <button
                    key={loader}
                    type="button"
                    className={styles.pill}
                    aria-pressed={committed.loader === loader}
                    onClick={() => {
                      commit({ loader });
                    }}
                  >
                    {loader === '' ? t.anyLoader : loaderName(loader)}
                  </button>
                ))}
              </div>
            </div>
          )}
          <div className={styles.pillGroup} role="group" aria-labelledby={sortLabel}>
            <span id={sortLabel} className={styles.pillLabel}>
              {t.sort}
            </span>
            <div className={styles.pills}>
              {SORTS.map((sort) => (
                <button
                  key={sort}
                  type="button"
                  className={styles.pill}
                  aria-pressed={committed.sort === sort}
                  onClick={() => {
                    sortBy(sort);
                  }}
                >
                  {t.sorts[sort]}
                </button>
              ))}
            </div>
          </div>
        </div>
        {instance !== null && <Switch label={c.fitsOnly} checked={fits} onChange={fitsToggle} />}
        {(committed.categories.length > 0 || committed.collection !== null) && (
          <div className={styles.applied} role="group" aria-label={c.applied}>
            {committed.collection !== null && (
              <span className={styles.appliedChip}>
                <bdi>{collectionTitle(committed.collection)}</bdi>
                <button
                  type="button"
                  className={styles.chipRemove}
                  aria-label={c.remove({ name: collectionTitle(committed.collection) })}
                  title={c.remove({ name: collectionTitle(committed.collection) })}
                  onClick={() => {
                    commit({ collection: null });
                    heading.current?.focus();
                  }}
                >
                  <Icon name="x" size={14} />
                </button>
              </span>
            )}
            {committed.categories.map((id) => {
              const category = categoryOf(kind, id);
              const name = categoryName(id);
              return (
                <span key={id} className={styles.appliedChip}>
                  {category !== null && (
                    <Icon
                      name={category.icon}
                      size={14}
                      className={category.mirror === true ? styles.mirror : undefined}
                    />
                  )}
                  {name}
                  <button
                    type="button"
                    className={styles.chipRemove}
                    aria-label={c.remove({ name })}
                    title={c.remove({ name })}
                    onClick={() => {
                      toggleCategory(id);
                      heading.current?.focus();
                    }}
                  >
                    <Icon name="x" size={14} />
                  </button>
                </span>
              );
            })}
            <button type="button" className={styles.linkButton} onClick={reset}>
              {c.reset}
            </button>
          </div>
        )}

        {browsing && (
          <CollectionsStrip
            kind={kind}
            game={game === '' ? null : game}
            loader={committed.loader === '' ? null : committed.loader}
            instance={instance}
            onPick={pickCollection}
          />
        )}

        <section className={styles.block} aria-labelledby={headingId}>
          <div className={styles.blockHead}>
            <h3 id={headingId} ref={heading} tabIndex={-1} className={styles.blockTitle}>
              {title}
            </h3>
            {pending === 'first' && <span className={styles.spinner} aria-hidden="true" />}
            <span className={styles.muted} role="status">
              {status}
            </span>
          </div>
          {error !== null && (
            <Notice tone="danger">
              <span className={styles.noticeRow}>
                {error}
                <Button
                  size="small"
                  onClick={() => {
                    search(committed);
                    // The notice and its button go while the search runs.
                    heading.current?.focus();
                  }}
                >
                  {c.tryAgain}
                </Button>
              </span>
            </Notice>
          )}
          {page === null && pending === 'first' && (
            <ul className={styles.tiles} aria-hidden="true">
              <TileSkeletons count={6} />
            </ul>
          )}
          {page !== null && page.hits.length > 0 && (
            <ul ref={tiles} className={styles.tiles} aria-busy={pending === 'first' || undefined}>
              {page.hits.map((hit, index) => (
                <Fragment key={hit.id}>
                  {index === resumeAt && mark !== null && (
                    <li className={styles.resume}>
                      <span className={[styles.chip, styles.chipSuccess].join(' ')}>
                        <Icon name="pin" size={14} />
                        {c.resume({ when: mark.when })}
                      </span>
                    </li>
                  )}
                  <li>
                    <CatalogTile
                      hit={hit}
                      busy={adding === hit.id}
                      onAdd={
                        canAdd && installable(hit.kind)
                          ? () => {
                              setAdding(hit.id);
                              void onQuickAdd(hit).finally(() => {
                                setAdding(null);
                              });
                            }
                          : undefined
                      }
                      onOpen={() => {
                        onOpen(hit.id);
                      }}
                    />
                  </li>
                </Fragment>
              ))}
              {pending === 'more' && <TileSkeletons count={3} />}
            </ul>
          )}
          {page !== null && page.hits.length === 0 && !loading && error === null && (
            <div className={styles.empty}>
              <p className={styles.emptyTitle}>{t.nothing}</p>
              <p className={styles.muted}>{c.nothingHint}</p>
              <div className={styles.emptyActions}>
                {!browsing && <Button onClick={reset}>{c.reset}</Button>}
                {fits && (
                  <Button
                    onClick={() => {
                      fitsToggle(false);
                      // This block goes while the wider search runs.
                      heading.current?.focus();
                    }}
                  >
                    {c.showAll}
                  </Button>
                )}
              </div>
              <p className={styles.muted}>{t.notInCatalog}</p>
              <Button
                variant="primary"
                onClick={() => {
                  onAddToCatalog(committed.text.trim());
                }}
              >
                {c.askCatalog}
              </Button>
            </div>
          )}
          {page !== null && page.hits.length > 0 && (
            <div className={styles.loadRow} aria-live="polite">
              {pending === 'more' ? (
                <>
                  <span className={styles.spinner} aria-hidden="true" />
                  {c.loadingMore({
                    shown: number(page.hits.length),
                    total: number(page.total),
                  })}
                </>
              ) : moreFailed ? (
                <>
                  {c.loadFailed}
                  <Button
                    onClick={() => {
                      // The button goes while the page comes: the list's last project keeps the focus.
                      const names = tiles.current?.querySelectorAll<HTMLElement>('h4 button');
                      names?.[names.length - 1]?.focus();
                      loadMore();
                    }}
                  >
                    {c.tryAgain}
                  </Button>
                </>
              ) : more && !watching ? (
                <Button onClick={loadMore}>{t.more}</Button>
              ) : page.hits.length >= MAX_HITS && page.total > MAX_HITS ? (
                c.capped({ count: number(MAX_HITS) })
              ) : null}
            </div>
          )}
          <div ref={end} className={styles.sentinel} />
        </section>
        <BackToTop target={searchBox} />
      </div>
    </>
  );
}
