import { useCallback, useEffect, useRef, useState } from 'react';
import { Button } from '../../components/Button';
import { Field } from '../../components/Field';
import { Notice } from '../../components/Notice';
import { StatusBadge } from '../../components/StatusBadge';
import forms from '../../components/forms.module.css';
import type {
  InstanceSummary,
  ModsHit,
  ModsKind,
  ModsSearchPage,
  ModsSort,
} from '../../lib/contracts';
import { describeIpcError, ipc } from '../../lib/ipc';
import { strings } from '../../lib/strings';
import { compact, dateOf, loaderOf, loadersFor, ourPicture } from './helpers';
import styles from './Mods.module.css';

const PAGE = 20;
const SORTS: ModsSort[] = ['relevance', 'downloads', 'trending', 'updated', 'new', 'title'];

interface Filters {
  text: string;
  game: string;
  loader: string;
  sort: ModsSort;
}

interface Props {
  kind: ModsKind;
  instance: InstanceSummary | null;
  onOpen: (projectId: string) => void;
  onAddToCatalog: (text: string) => void;
}

/** One project in the results. */
function HitCard({ hit, onOpen }: { hit: ModsHit; onOpen: () => void }) {
  const t = strings.mods;
  const icon = ourPicture(hit.icon);
  return (
    <li>
      <button type="button" className={styles.card} onClick={onOpen}>
        {icon !== null ? (
          <img className={styles.icon} src={icon} alt="" loading="lazy" />
        ) : (
          <span className={styles.icon} aria-hidden="true" />
        )}
        <span className={styles.cardBody}>
          <span className={styles.cardTitle}>
            {hit.title || hit.slug}
            {hit.status === 'importing' && <StatusBadge tone="warning" label={t.importing} />}
          </span>
          {hit.author !== null && hit.author !== '' && (
            <span className={styles.muted}>{t.by({ author: hit.author })}</span>
          )}
          <span className={styles.summary}>{hit.summary}</span>
          <span className={styles.meta}>
            {t.downloads({ count: compact(hit.downloads) })}
            {hit.updated > 0 && ` · ${t.updatedOn({ date: dateOf(hit.updated) })}`}
            {hit.sources.map((source) => (
              <span key={source} className={styles.source}>
                {t.sources[source]}
              </span>
            ))}
          </span>
        </span>
      </button>
    </li>
  );
}

/**
 * Search in one kind of project. With an installation chosen, the filters
 * start at its game version and loader.
 */
export function SearchSection({ kind, instance, onOpen, onAddToCatalog }: Props) {
  const t = strings.mods;
  const loaders = loadersFor(kind);
  const [draft, setDraft] = useState<Filters>(() => ({
    text: '',
    game: instance?.versionId ?? '',
    loader: loaderOf(kind, instance),
    sort: 'downloads',
  }));
  const [page, setPage] = useState<ModsSearchPage | null>(null);
  const [error, setError] = useState<string | null>(null);
  // The first search runs on open.
  const [loading, setLoading] = useState(true);
  // Answers to older searches are dropped.
  const generation = useRef(0);

  // State changes only when the answer comes.
  const fetchPage = useCallback(
    (filters: Filters, offset: number) => {
      const current = ++generation.current;
      void ipc
        .modsSearch({
          text: filters.text.trim(),
          kind,
          game: filters.game.trim() === '' ? null : filters.game.trim(),
          loader: filters.loader === '' ? null : filters.loader,
          category: null,
          sort: filters.text.trim() === '' && filters.sort === 'relevance' ? null : filters.sort,
          offset,
          limit: PAGE,
        })
        .then((found) => {
          if (current !== generation.current) return;
          setError(null);
          setPage((before) =>
            offset === 0 || before === null
              ? found
              : { total: found.total, hits: [...before.hits, ...found.hits] },
          );
        })
        .catch((e: unknown) => {
          if (current === generation.current) setError(describeIpcError(e));
        })
        .finally(() => {
          if (current === generation.current) setLoading(false);
        });
    },
    [kind],
  );

  const search = (filters: Filters, offset: number) => {
    setLoading(true);
    setError(null);
    fetchPage(filters, offset);
  };

  useEffect(() => {
    fetchPage(draft, 0);
    // The first search only; later ones start from the form.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [fetchPage]);

  const change = (patch: Partial<Filters>, now: boolean) => {
    const next = { ...draft, ...patch };
    setDraft(next);
    if (now) search(next, 0);
  };

  return (
    <div className={styles.section}>
      <form
        className={styles.controls}
        role="search"
        onSubmit={(event) => {
          event.preventDefault();
          search(draft, 0);
        }}
      >
        <Field label={t.search}>
          {(props) => (
            <input
              {...props}
              type="search"
              className={forms.input}
              value={draft.text}
              maxLength={200}
              placeholder={t.searchPlaceholder}
              onChange={(event) => {
                change({ text: event.target.value }, false);
              }}
            />
          )}
        </Field>
        <Field label={t.gameVersion}>
          {(props) => (
            <input
              {...props}
              className={[forms.input, styles.short].filter(Boolean).join(' ')}
              value={draft.game}
              maxLength={40}
              placeholder={t.anyVersion}
              dir="ltr"
              onChange={(event) => {
                change({ game: event.target.value }, false);
              }}
            />
          )}
        </Field>
        {loaders.length > 0 && (
          <Field label={t.loader}>
            {(props) => (
              <select
                {...props}
                className={forms.select}
                value={draft.loader}
                onChange={(event) => {
                  change({ loader: event.target.value }, true);
                }}
              >
                <option value="">{t.anyLoader}</option>
                {loaders.map((loader) => (
                  <option key={loader} value={loader}>
                    {t.loaderNames[loader as keyof typeof t.loaderNames]}
                  </option>
                ))}
              </select>
            )}
          </Field>
        )}
        <Field label={t.sort}>
          {(props) => (
            <select
              {...props}
              className={forms.select}
              value={draft.sort}
              onChange={(event) => {
                change({ sort: event.target.value as ModsSort }, true);
              }}
            >
              {SORTS.map((sort) => (
                <option key={sort} value={sort}>
                  {t.sorts[sort]}
                </option>
              ))}
            </select>
          )}
        </Field>
        <Button type="submit" variant="primary" disabled={loading}>
          {t.find}
        </Button>
      </form>

      {error !== null && <Notice tone="danger">{error}</Notice>}
      {page !== null && (
        <p className={styles.muted} role="status">
          {page.hits.length === 0 ? t.nothing : t.results({ count: page.total })}
        </p>
      )}
      {page === null && loading && <p role="status">{t.loading}</p>}
      {page !== null && page.hits.length > 0 && (
        <ul className={styles.cards}>
          {page.hits.map((hit) => (
            <HitCard
              key={hit.id}
              hit={hit}
              onOpen={() => {
                onOpen(hit.id);
              }}
            />
          ))}
        </ul>
      )}
      <div className={forms.actions}>
        {page !== null && page.hits.length < page.total && (
          <Button
            disabled={loading}
            onClick={() => {
              search(draft, page.hits.length);
            }}
          >
            {loading ? t.loading : t.more}
          </Button>
        )}
      </div>
      <div className={styles.missing}>
        <p>{t.notInCatalog}</p>
        <Button
          onClick={() => {
            onAddToCatalog(draft.text.trim());
          }}
        >
          {t.addToCatalog}
        </Button>
      </div>
    </div>
  );
}
