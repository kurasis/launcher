import { useCallback, useEffect, useId, useRef, useState } from 'react';
import { Icon } from '../../components/Icon';
import { Notice } from '../../components/Notice';
import forms from '../../components/forms.module.css';
import type { DocNode, InstanceSummary, ModsProject, ModsVersion } from '../../lib/contracts';
import { formatBytes } from '../../lib/format';
import { describeIpcError, ipc } from '../../lib/ipc';
import { numberLocale, strings } from '../../lib/strings';
import { Dialog } from '../../components/Dialog';
import { cannotAdd, installable, installKind, isolate, packArchive, validGame } from './helpers';
import { dayOf, fits, pageOf, shortDayOf } from './versions';
import { RichText } from './RichText';
import catalog from './Catalog.module.css';
import styles from './Project.module.css';

/** Rows asked for at a time. */
const PAGE = 50;
/** Rows the table holds at most; the filters narrow the rest. */
const MAX_ROWS = 1000;
/** Game versions named in a row; the rest are counted. */
const GAMES_SHOWN = 3;
/** Loaders offered as pills at most. */
const LOADERS_SHOWN = 8;
/** How far below the table's end the next page is asked for. */
const AHEAD = '200px';
const CHANNELS = ['release', 'beta', 'alpha'] as const;
type Channel = (typeof CHANNELS)[number];

interface Filters {
  game: string;
  loader: string;
  channel: '' | Channel;
}

interface Table {
  /** The filters the rows answer. */
  for: string;
  rows: ModsVersion[];
  total: number;
  /** A page brought nothing new: there is no more to ask. */
  ended: boolean;
}

export interface VersionsSummary {
  /** All its versions. */
  total: number;
  /** Game versions (newest first) and loaders of all of them: the filters' choices. */
  games: string[];
  loaders: string[];
}

const keyOf = (filters: Filters) => JSON.stringify([filters.game, filters.loader, filters.channel]);

/** A loader's name; a data pack is one too. */
function loaderName(loader: string): string {
  if (loader === 'datapack') return strings.mods.project.loaderDatapack;
  const names = strings.mods.loaderNames as Record<string, string | undefined>;
  return names[loader] ?? loader;
}

/** Game versions newest first, releases before snapshots. */
function byGame(a: string, b: string): number {
  const release = /^\d+(\.\d+)+$/;
  const ar = release.test(a);
  if (ar !== release.test(b)) return ar ? -1 : 1;
  return b.localeCompare(a, 'en', { numeric: true });
}

/** A version's details and what is new in it, with "Save to the catalog". */
function VersionDialog({
  version,
  onClose,
  onLink,
}: {
  version: ModsVersion;
  onClose: () => void;
  onLink: (url: string) => void;
}) {
  const t = strings.mods.project;
  const [changelog, setChangelog] = useState<DocNode[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [asked, setAsked] = useState(version.saved === 'all');
  const held = version.files.some((file) => file.held);
  useEffect(() => {
    let live = true;
    ipc
      .modsVersion(version.id)
      .then((full) => {
        if (live) setChangelog(Array.isArray(full.changelog) ? full.changelog : []);
      })
      .catch((e: unknown) => {
        if (live) setError(describeIpcError(e));
      });
    return () => {
      live = false;
    };
  }, [version.id]);
  return (
    <Dialog title={t.whatsNewIn({ number: isolate(version.number) })} onClose={onClose} wide>
      {version.name !== '' && version.name !== version.number && (
        <p className={styles.hint} dir="auto">
          {version.name}
        </p>
      )}
      <dl className={styles.facts}>
        <dt>{t.channel}</dt>
        <dd>{t.channels[version.channel]}</dd>
        {version.published > 0 && (
          <>
            <dt>{t.facts.published}</dt>
            <dd>{dayOf(version.published)}</dd>
          </>
        )}
        {version.games.length > 0 && (
          <>
            <dt>{t.facts.games}</dt>
            <dd>
              {/* Left to right, at the start of the line in Arabic too. */}
              <span dir="ltr">{[...version.games].sort(byGame).join(', ')}</span>
            </dd>
          </>
        )}
        {version.loaders.length > 0 && (
          <>
            <dt>{t.facts.loaders}</dt>
            <dd>{version.loaders.map(loaderName).join(', ')}</dd>
          </>
        )}
        {version.files.length > 0 && (
          <>
            <dt>{t.facts.files}</dt>
            <dd>
              <ul className={styles.files}>
                {version.files.map((file) => (
                  <li key={file.name}>
                    <bdi dir="ltr">{file.name}</bdi>
                    {file.size > 0 && ` · ${formatBytes(file.size)}`}
                  </li>
                ))}
              </ul>
            </dd>
          </>
        )}
      </dl>
      {held ? (
        <Notice tone="danger">{t.held}</Notice>
      ) : (
        version.saved !== 'all' && (
          <div className={styles.keep}>
            <p className={styles.hint}>{asked ? t.fetchAsked : t.notKept}</p>
            {!asked && (
              <button
                type="button"
                className={[catalog.smallButton, styles.ghostButton].join(' ')}
                onClick={() => {
                  setError(null);
                  ipc
                    .modsFetchVersion(version.id)
                    .then(() => {
                      setAsked(true);
                    })
                    .catch((e: unknown) => {
                      setError(describeIpcError(e));
                    });
                }}
              >
                {t.keep}
              </button>
            )}
          </div>
        )
      )}
      {version.goneAt !== null && <p className={styles.hint}>{t.versionGone}</p>}
      {error !== null && <Notice tone="danger">{error}</Notice>}
      <div className={styles.changelog}>
        {changelog === null ? (
          error === null && (
            <p role="status" className={styles.hint}>
              {strings.mods.loading}
            </p>
          )
        ) : changelog.length === 0 ? (
          <p className={styles.hint}>{t.noChanges}</p>
        ) : (
          <RichText nodes={changelog} onLink={onLink} />
        )}
      </div>
      <div className={styles.dialogActions}>
        <span className={catalog.spacer} />
        <button
          type="button"
          className={[catalog.smallButton, styles.ghostButton].join(' ')}
          onClick={onClose}
        >
          {t.close}
        </button>
      </div>
    </Dialog>
  );
}

interface Props {
  project: ModsProject;
  instance: InstanceSummary | null;
  summary: VersionsSummary;
  /** Versions made for the installation's game version; null when that does not apply. */
  fit: { rows: ModsVersion[]; capped: boolean } | 'loading' | null;
  /** Opened from the data packs: a mod's data pack versions are shown. */
  datapacks: boolean;
  /** The version of this project the installation holds. */
  installedVersion: string | null;
  onAdd: (version: ModsVersion) => void;
  onInstallPack: (version: ModsVersion) => void;
  onLink: (url: string) => void;
}

/**
 * Every version in a table that scrolls inside its panel (D-072): the ones made for
 * the installation first, then the rest, dimmed; filters, "Only for …", and the next
 * page as the end of the table comes near.
 */
export function ProjectVersions({
  project,
  instance,
  summary,
  fit,
  datapacks,
  installedVersion,
  onAdd,
  onInstallPack,
  onLink,
}: Props) {
  const t = strings.mods.project;
  const c = t.columns;
  const headingId = useId();
  const gameId = useId();
  const loaderLabel = useId();
  const channelLabel = useId();
  const kind = project.kind;
  const fitApplies = fit !== null && instance !== null;
  const fitRows = fit === null || fit === 'loading' ? null : fit;
  const games = summary.games.filter((game) => game !== '' && validGame(game));
  const loaders = summary.loaders.filter((loader) => loader !== '').slice(0, LOADERS_SHOWN);
  const [filters, setFilters] = useState<Filters>(() => ({
    game: '',
    loader: datapacks && kind === 'mod' && loaders.includes('datapack') ? 'datapack' : '',
    channel: '',
  }));
  const [only, setOnly] = useState(false);
  const [table, setTable] = useState<Table | null>(null);
  const [pending, setPending] = useState<'first' | 'more' | null>('first');
  const [error, setError] = useState<string | null>(null);
  const [moreFailed, setMoreFailed] = useState(false);
  const [attempt, setAttempt] = useState(0);
  const [details, setDetails] = useState<ModsVersion | null>(null);
  const asked = useRef(0);
  const box = useRef<HTMLDivElement>(null);
  // In state: the table and its end come and go with the loading rows.
  const [end, setEnd] = useState<HTMLDivElement | null>(null);

  const fetchPage = useCallback(
    (next: Filters, offset: number) => {
      const ticket = ++asked.current;
      ipc
        .modsVersionsPage(project.id, {
          game: next.game === '' ? null : next.game,
          loader: next.loader === '' ? null : next.loader,
          channel: next.channel === '' ? null : next.channel,
          offset,
          limit: PAGE,
        })
        .then((answer) => {
          if (ticket !== asked.current) return;
          const found = pageOf(answer);
          const list = found.versions.slice(0, PAGE);
          setTable((now) => {
            if (offset === 0 || now === null) {
              return {
                for: keyOf(next),
                rows: list,
                total: found.total,
                ended: list.length === 0 || list.length >= found.total,
              };
            }
            const seen = new Set(now.rows.map((row) => row.id));
            const fresh = list.filter((row) => !seen.has(row.id));
            const rows = [...now.rows, ...fresh];
            return {
              ...now,
              rows,
              total: found.total,
              ended: fresh.length === 0 || rows.length >= found.total,
            };
          });
        })
        .catch((e: unknown) => {
          if (ticket !== asked.current) return;
          if (offset === 0) {
            setError(describeIpcError(e));
            setTable(null);
          } else {
            setMoreFailed(true);
          }
        })
        .finally(() => {
          if (ticket === asked.current) setPending(null);
        });
    },
    [project.id],
  );

  // The first page for the filters; a change of them asks again.
  useEffect(() => {
    fetchPage(filters, 0);
  }, [fetchPage, filters, attempt]);

  const change = (next: Partial<Filters>) => {
    setFilters({ ...filters, ...next });
    setPending('first');
    setError(null);
    setMoreFailed(false);
  };
  const showAll = () => {
    setOnly(false);
    if (filters.game === '' && filters.loader === '' && filters.channel === '') return;
    change({ game: '', loader: '', channel: '' });
  };

  const current = table !== null && table.for === keyOf(filters) ? table : null;
  const loading = pending === 'first' || current === null || fit === 'loading';
  // Only while there is an installation to fit.
  const onlyFit = only && fitApplies;
  const more =
    !onlyFit &&
    current !== null &&
    !current.ended &&
    current.rows.length < Math.min(current.total, MAX_ROWS);
  const loadMore = useCallback(() => {
    if (!more || pending !== null || error !== null) return;
    setPending('more');
    setMoreFailed(false);
    fetchPage(filters, current.rows.length);
  }, [more, pending, error, current, fetchPage, filters]);
  const watching = typeof IntersectionObserver !== 'undefined';
  useEffect(() => {
    const root = box.current;
    if (!watching || end === null || !more || pending !== null || moreFailed) return;
    const observer = new IntersectionObserver(
      (entries) => {
        if (entries.some((entry) => entry.isIntersecting)) loadMore();
      },
      { root, rootMargin: `0px 0px ${AHEAD} 0px` },
    );
    observer.observe(end);
    return () => {
      observer.disconnect();
    };
  }, [watching, end, more, pending, moreFailed, loadMore]);

  const matches = (version: ModsVersion) =>
    (filters.game === '' || version.games.includes(filters.game)) &&
    (filters.loader === '' || version.loaders.includes(filters.loader)) &&
    (filters.channel === '' || version.channel === filters.channel);
  const fitting =
    fitApplies && fitRows !== null
      ? fitRows.rows.filter((version) => matches(version) && fits(kind, version, instance))
      : [];
  const fittingIds = new Set(fitting.map((version) => version.id));
  const rest = onlyFit
    ? []
    : (current?.rows ?? []).filter((version) => !fittingIds.has(version.id));
  const rows = [...fitting, ...rest];
  const total = current?.total ?? 0;
  const status = loading
    ? strings.mods.loading
    : !fitApplies
      ? t.count({ count: total })
      : fitRows?.capped === true
        ? t.countFitMore({ count: total, fit: fitting.length })
        : t.countFit({ count: total, fit: fitting.length });
  const name = isolate(instance?.displayName ?? '');

  const action = (version: ModsVersion) => {
    if (version.id === installedVersion) {
      return (
        <span className={[catalog.chip, catalog.chipSuccess].join(' ')}>{t.installedRow}</span>
      );
    }
    const number = isolate(version.number);
    if (kind === 'modpack') {
      const ready =
        version.files.some((file) => packArchive(file.name)) &&
        !version.files.some((file) => file.held);
      if (!ready) return <span aria-hidden="true">—</span>;
      const label = t.installRow({ number });
      return (
        <button
          type="button"
          className={[catalog.smallButton, catalog.iconButton, styles.rowButton].join(' ')}
          aria-label={label}
          title={label}
          onClick={() => {
            onInstallPack(version);
          }}
        >
          <Icon name="downloads" size={18} />
        </button>
      );
    }
    if (!installable(kind)) return null;
    const reason = cannotAdd(installKind(kind, version), version, instance);
    if (reason !== null) {
      return (
        <>
          <span aria-hidden="true" title={reason}>
            —
          </span>
          <span className={catalog.visuallyHidden}>{t.notFor({ reason })}</span>
        </>
      );
    }
    const label = t.addRow({ number });
    return (
      <button
        type="button"
        className={[catalog.smallButton, catalog.iconButton, styles.rowButton].join(' ')}
        aria-label={label}
        title={label}
        onClick={() => {
          onAdd(version);
        }}
      >
        <Icon name="plus" size={18} />
      </button>
    );
  };

  const file = (version: ModsVersion) =>
    version.files.some((f) => f.held) ? (
      <span className={[catalog.chip, styles.chipDanger].join(' ')}>{t.fileHeld}</span>
    ) : version.saved === 'all' ? (
      <span className={styles.outlineChip}>{t.kept}</span>
    ) : (
      <span className={[catalog.chip, styles.chipWarning].join(' ')} title={t.notKept}>
        {t.fileOnRequest}
      </span>
    );

  return (
    <section className={styles.panel} aria-labelledby={headingId}>
      <div className={styles.panelHead}>
        <h3 id={headingId} className={styles.panelTitle}>
          {t.versions}
        </h3>
        <span className={styles.count}>{summary.total.toLocaleString(numberLocale())}</span>
      </div>
      {summary.total === 0 ? (
        <p className={styles.hint}>{t.noVersions}</p>
      ) : (
        <>
          <div className={styles.filters}>
            {games.length > 1 && (
              <>
                <label htmlFor={gameId} className={catalog.visuallyHidden}>
                  {strings.mods.gameVersion}
                </label>
                <select
                  id={gameId}
                  className={[forms.select, styles.gameSelect].join(' ')}
                  value={filters.game}
                  onChange={(event) => {
                    const game = event.target.value;
                    if (game === '' || games.includes(game)) change({ game });
                  }}
                >
                  <option value="">{t.anyGame}</option>
                  {games.map((game) => (
                    <option key={game} value={game}>
                      {game}
                    </option>
                  ))}
                </select>
              </>
            )}
            {loaders.length > 1 && (
              <div className={catalog.pills} role="group" aria-labelledby={loaderLabel}>
                <span id={loaderLabel} className={catalog.visuallyHidden}>
                  {strings.mods.loader}
                </span>
                {['', ...loaders].map((loader) => (
                  <button
                    key={loader}
                    type="button"
                    className={catalog.pill}
                    aria-pressed={filters.loader === loader}
                    onClick={() => {
                      if (filters.loader !== loader) change({ loader });
                    }}
                  >
                    {loader === '' ? strings.mods.anyLoader : loaderName(loader)}
                  </button>
                ))}
              </div>
            )}
            <div className={catalog.pills} role="group" aria-labelledby={channelLabel}>
              <span id={channelLabel} className={catalog.visuallyHidden}>
                {t.channel}
              </span>
              {(['', ...CHANNELS] as const).map((channel) => (
                <button
                  key={channel}
                  type="button"
                  className={catalog.pill}
                  aria-pressed={filters.channel === channel}
                  onClick={() => {
                    if (filters.channel !== channel) change({ channel });
                  }}
                >
                  {channel === '' ? t.anyChannel : t.channels[channel]}
                </button>
              ))}
            </div>
            {fitApplies && (
              <label className={catalog.switch}>
                <input
                  type="checkbox"
                  role="switch"
                  checked={only}
                  onChange={(event) => {
                    setOnly(event.target.checked);
                  }}
                />
                {t.onlyFor({ name })}
              </label>
            )}
          </div>
          <p className={styles.hint} role="status">
            {status}
          </p>
          {error !== null ? (
            <div className={catalog.empty}>
              <p className={catalog.emptyTitle}>{t.loadFailed}</p>
              <p className={styles.hint}>{error}</p>
              <button
                type="button"
                className={[catalog.smallButton, catalog.soft].join(' ')}
                onClick={() => {
                  setError(null);
                  setPending('first');
                  setAttempt((n) => n + 1);
                }}
              >
                {strings.mods.catalog.tryAgain}
              </button>
            </div>
          ) : !loading && rows.length === 0 ? (
            <div className={catalog.empty}>
              <p className={catalog.emptyTitle}>
                {onlyFit && total > 0 ? t.noneFit({ name }) : t.noneMatch}
              </p>
              <button
                type="button"
                className={[catalog.smallButton, catalog.soft].join(' ')}
                onClick={showAll}
              >
                {t.showAll}
              </button>
            </div>
          ) : (
            <div
              ref={box}
              className={styles.tableBox}
              role="region"
              aria-label={t.listLabel}
              // Scrolled from the keyboard too.
              tabIndex={0}
            >
              <table
                className={styles.table}
                aria-labelledby={headingId}
                aria-busy={loading || undefined}
              >
                <thead>
                  <tr>
                    <th scope="col">{c.version}</th>
                    <th scope="col">{c.type}</th>
                    <th scope="col">{c.for}</th>
                    <th scope="col" className={styles.dateCell}>
                      {c.date}
                    </th>
                    <th scope="col">{c.file}</th>
                    <th scope="col">
                      <span className={catalog.visuallyHidden}>{c.action}</span>
                    </th>
                  </tr>
                </thead>
                <tbody>
                  {loading
                    ? Array.from({ length: 5 }, (_, index) => (
                        <tr key={index} className={styles.skeletonRow} aria-hidden="true">
                          <td colSpan={6}>
                            <span className={catalog.skeletonLine} />
                          </td>
                        </tr>
                      ))
                    : rows.map((version) => {
                        const dim = fitApplies && !fits(kind, version, instance);
                        const shownGames = [...version.games].sort(byGame);
                        return (
                          <tr key={version.id} className={dim ? styles.misfit : undefined}>
                            <th scope="row">
                              <button
                                type="button"
                                className={styles.versionButton}
                                aria-label={t.whatsNewIn({ number: isolate(version.number) })}
                                title={version.name === '' ? undefined : version.name}
                                onClick={() => {
                                  setDetails(version);
                                }}
                              >
                                <bdi dir="ltr">{version.number}</bdi>
                              </button>
                            </th>
                            <td>
                              <span
                                className={
                                  dim
                                    ? catalog.chip
                                    : version.channel === 'release'
                                      ? [catalog.chip, catalog.chipSuccess].join(' ')
                                      : [catalog.chip, styles.chipWarning].join(' ')
                                }
                              >
                                {t.channels[version.channel]}
                              </span>
                            </td>
                            <td>
                              <span className={styles.forGames}>
                                <span dir="ltr">
                                  {shownGames.slice(0, GAMES_SHOWN).join(', ')}
                                  {shownGames.length > GAMES_SHOWN &&
                                    ` +${String(shownGames.length - GAMES_SHOWN)}`}
                                </span>
                              </span>
                              {version.loaders.length > 0 && (
                                <span className={styles.forLoaders}>
                                  {version.loaders.map(loaderName).join(', ')}
                                </span>
                              )}
                            </td>
                            <td className={styles.dateCell}>{shortDayOf(version.published)}</td>
                            <td>{dim ? null : file(version)}</td>
                            <td className={styles.actionCell}>{action(version)}</td>
                          </tr>
                        );
                      })}
                </tbody>
              </table>
              {!loading && (
                <>
                  {pending === 'more' && (
                    <p className={styles.loadRow} role="status">
                      <span className={catalog.spinner} aria-hidden="true" />
                      {t.loadingMore}
                    </p>
                  )}
                  {moreFailed && (
                    <div className={styles.loadRow}>
                      <span>{t.moreFailed}</span>
                      <button
                        type="button"
                        className={[catalog.smallButton, catalog.soft].join(' ')}
                        onClick={() => {
                          setMoreFailed(false);
                          loadMore();
                        }}
                      >
                        {strings.mods.catalog.tryAgain}
                      </button>
                    </div>
                  )}
                  {more && !watching && pending === null && !moreFailed && (
                    <div className={styles.loadRow}>
                      <button
                        type="button"
                        className={[catalog.smallButton, catalog.soft].join(' ')}
                        onClick={loadMore}
                      >
                        {t.loadMore}
                      </button>
                    </div>
                  )}
                  {!onlyFit &&
                    current.rows.length >= MAX_ROWS &&
                    current.total > current.rows.length && (
                      <p className={styles.loadRow}>{t.capped({ count: MAX_ROWS })}</p>
                    )}
                  <div ref={setEnd} className={catalog.sentinel} />
                </>
              )}
            </div>
          )}
        </>
      )}
      {details !== null && (
        <VersionDialog
          version={details}
          onLink={onLink}
          onClose={() => {
            setDetails(null);
          }}
        />
      )}
    </section>
  );
}
