import { useEffect, useMemo, useState } from 'react';
import { Button } from '../../components/Button';
import { Field } from '../../components/Field';
import { Notice } from '../../components/Notice';
import { StatusBadge } from '../../components/StatusBadge';
import forms from '../../components/forms.module.css';
import type {
  InstanceSummary,
  ModsProject,
  ModsReportReason,
  ModsVersion,
  DocNode,
} from '../../lib/contracts';
import { describeIpcError, ipc } from '../../lib/ipc';
import { strings } from '../../lib/strings';
import {
  cannotAdd,
  compact,
  dateOf,
  installable,
  installKind,
  loaderOf,
  ourPicture,
  packArchive,
} from './helpers';
import { RichText } from './RichText';
import styles from './Mods.module.css';

const SHOWN = 30;
const CHANNELS = ['release', 'beta', 'alpha'] as const;

/** Game versions newest first, releases before snapshots. */
function byGame(a: string, b: string): number {
  const release = /^\d+(\.\d+)+$/;
  const ar = release.test(a);
  if (ar !== release.test(b)) return ar ? -1 : 1;
  return b.localeCompare(a, 'en', { numeric: true });
}

/** What the versions run on, to filter by. */
function choices(versions: ModsVersion[]): { games: string[]; loaders: string[] } {
  const games = new Set<string>();
  const loaders = new Set<string>();
  for (const v of versions) {
    for (const g of v.games) games.add(g);
    for (const l of v.loaders) loaders.add(l);
  }
  return { games: [...games].sort(byGame), loaders: [...loaders].sort() };
}

const REASONS: ModsReportReason[] = ['malware', 'stolen', 'inappropriate', 'broken', 'other'];

/** A player's report of another player's pack, for the catalog's owner (D-066 stage 6). */
function ReportForm({ projectId, onClose }: { projectId: string; onClose: () => void }) {
  const t = strings.mods.project;
  const [reason, setReason] = useState<ModsReportReason>('broken');
  const [text, setText] = useState('');
  const [working, setWorking] = useState(false);
  const [sent, setSent] = useState(false);
  const [error, setError] = useState<string | null>(null);
  if (sent) return <Notice tone="info">{t.reportSent}</Notice>;
  return (
    <form
      className={forms.form}
      aria-label={t.report}
      onSubmit={(event) => {
        event.preventDefault();
        setWorking(true);
        setError(null);
        ipc
          .modsReport(projectId, reason, text)
          .then(() => {
            setSent(true);
          })
          .catch((e: unknown) => {
            setError(describeIpcError(e));
          })
          .finally(() => {
            setWorking(false);
          });
      }}
    >
      <Field label={t.reportReason}>
        {(props) => (
          <select
            {...props}
            className={forms.select}
            value={reason}
            disabled={working}
            onChange={(e) => {
              const picked = REASONS.find((r) => r === e.target.value);
              if (picked !== undefined) setReason(picked);
            }}
          >
            {REASONS.map((r) => (
              <option key={r} value={r}>
                {t.reportReasons[r]}
              </option>
            ))}
          </select>
        )}
      </Field>
      <Field label={t.reportText}>
        {(props) => (
          <textarea
            {...props}
            className={forms.input}
            value={text}
            maxLength={500}
            rows={3}
            disabled={working}
            onChange={(e) => {
              setText(e.target.value);
            }}
          />
        )}
      </Field>
      {error !== null && <Notice tone="danger">{error}</Notice>}
      <div className={forms.actions}>
        <Button type="submit" variant="primary" disabled={working}>
          {t.reportSend}
        </Button>
        <Button disabled={working} onClick={onClose}>
          {t.reportCancel}
        </Button>
      </div>
    </form>
  );
}

interface Props {
  /** Our ID or the slug. */
  projectRef: string;
  instance: InstanceSummary | null;
  onBack: () => void;
  onLink: (url: string) => void;
  onAdd: (project: ModsProject, version: ModsVersion) => void;
  onInstallPack: (project: ModsProject, version: ModsVersion) => void;
  /** Opened from the data packs: a mod's data pack versions show first. */
  datapacks?: boolean;
}

/** One version: what it runs on, whether the catalog keeps it, and actions. */
function VersionRow({
  version,
  project,
  instance,
  onAdd,
  onInstallPack,
  onLink,
}: {
  version: ModsVersion;
  project: ModsProject;
  instance: InstanceSummary | null;
  onAdd: () => void;
  onInstallPack: () => void;
  onLink: (url: string) => void;
}) {
  const t = strings.mods.project;
  const [asked, setAsked] = useState(version.saved === 'all');
  const [error, setError] = useState<string | null>(null);
  const [changelog, setChangelog] = useState<DocNode[] | null>(null);
  const [open, setOpen] = useState(false);
  const file = version.files.find((f) => f.primary) ?? version.files[0];
  // The catalog's malware check holds a file back until the owner decides.
  const held = version.files.some((f) => f.held);
  // A Modrinth or CurseForge pack.
  const pack = project.kind === 'modpack' && version.files.some((f) => packArchive(f.name));
  // Why the version cannot go in, shown on hover; a disabled button gets no
  // pointer events everywhere, so the wrapper carries it too.
  const blocked = cannotAdd(installKind(project.kind, version), version, instance);
  const packBlocked = held ? t.cannotAdd.held : null;
  return (
    <li className={styles.version}>
      <div className={styles.versionHead}>
        <span className={styles.versionName}>
          <bdi>{version.name || version.number}</bdi>
          {version.channel !== 'release' && (
            <StatusBadge tone="warning" label={t.channels[version.channel]} />
          )}
          {version.saved === 'all' && !held && <StatusBadge tone="success" label={t.kept} />}
        </span>
        <span className={styles.muted}>
          <bdi dir="ltr">{version.games.slice(0, 6).join(', ')}</bdi>
          {version.games.length > 6 && ` ${t.andMore({ count: version.games.length - 6 })}`}
          {version.loaders.length > 0 && ` · ${version.loaders.join(', ')}`}
          {` · ${dateOf(version.published)}`}
          {file !== undefined && ` · ${file.name}`}
        </span>
      </div>
      {held ? (
        <Notice tone="danger">{t.held}</Notice>
      ) : (
        version.saved !== 'all' && <p className={styles.note}>{asked ? t.fetchAsked : t.notKept}</p>
      )}
      {version.goneAt !== null && <p className={styles.note}>{t.versionGone}</p>}
      {error !== null && <Notice tone="danger">{error}</Notice>}
      <div className={forms.actions}>
        {installable(project.kind) && (
          <span title={blocked ?? undefined}>
            <Button
              variant="primary"
              disabled={blocked !== null}
              title={blocked ?? undefined}
              onClick={onAdd}
            >
              {t.add}
            </Button>
          </span>
        )}
        {pack && (
          <span title={packBlocked ?? undefined}>
            <Button
              variant="primary"
              disabled={packBlocked !== null}
              title={packBlocked ?? undefined}
              onClick={onInstallPack}
            >
              {strings.mods.pack.install}
            </Button>
          </span>
        )}
        {!asked && (
          <Button
            onClick={() => {
              setError(null);
              void ipc
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
          </Button>
        )}
        <Button
          aria-expanded={open}
          onClick={() => {
            setOpen(!open);
            if (changelog === null) {
              void ipc
                .modsVersion(version.id)
                .then((full) => {
                  setChangelog(full.changelog);
                })
                .catch((e: unknown) => {
                  setError(describeIpcError(e));
                });
            }
          }}
        >
          {t.changes}
        </Button>
      </div>
      {open &&
        (changelog === null ? (
          <p role="status">{strings.mods.loading}</p>
        ) : changelog.length === 0 ? (
          <p className={styles.muted}>{t.noChanges}</p>
        ) : (
          <RichText nodes={changelog} onLink={onLink} />
        ))}
    </li>
  );
}

/** A project's page: description, gallery and versions (D-066). */
export function ProjectPage({
  projectRef,
  instance,
  onBack,
  onLink,
  onAdd,
  onInstallPack,
  datapacks = false,
}: Props) {
  const t = strings.mods.project;
  const [project, setProject] = useState<ModsProject | null>(null);
  // Every version of the project, newest first.
  const [versions, setVersions] = useState<ModsVersion[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  // Filters; empty means any. The chosen installation's game and loader at
  // first, once the versions show which there are.
  const [filters, setFilters] = useState<{
    game: string;
    loader: string;
    channel: '' | (typeof CHANNELS)[number];
    oldest: boolean;
  } | null>(null);
  const [shown, setShown] = useState(SHOWN);
  const [reporting, setReporting] = useState(false);

  // The parent keys this page by project, so it starts empty.
  useEffect(() => {
    let live = true;
    void ipc
      .modsProject(projectRef)
      .then((found) => {
        if (live) setProject(found);
      })
      .catch((e: unknown) => {
        if (live) setError(describeIpcError(e));
      });
    return () => {
      live = false;
    };
  }, [projectRef]);

  const projectId = project?.id ?? null;
  useEffect(() => {
    if (projectId === null) return;
    let live = true;
    void ipc
      .modsVersions(projectId, null, null)
      .then((found) => {
        if (live) setVersions(found);
      })
      .catch((e: unknown) => {
        if (live) setError(describeIpcError(e));
      });
    return () => {
      live = false;
    };
  }, [projectId]);

  const options = useMemo(() => choices(versions ?? []), [versions]);
  // A modpack makes an installation of its own, so nothing is chosen for one.
  const fitting = instance !== null && project !== null && project.kind !== 'modpack';
  const initial = {
    game: fitting && options.games.includes(instance.versionId) ? instance.versionId : '',
    loader: (() => {
      if (datapacks && project?.kind === 'mod' && options.loaders.includes('datapack')) {
        return 'datapack';
      }
      const loader = fitting ? loaderOf(project.kind, instance) : '';
      return options.loaders.includes(loader) ? loader : '';
    })(),
    channel: '' as const,
    oldest: false,
  };
  const chosen = filters ?? initial;
  const change = (next: Partial<typeof chosen>) => {
    setFilters({ ...chosen, ...next });
    setShown(SHOWN);
  };
  const matching = useMemo(() => {
    const list = (versions ?? []).filter(
      (v) =>
        (chosen.game === '' || v.games.includes(chosen.game)) &&
        (chosen.loader === '' || v.loaders.includes(chosen.loader)) &&
        (chosen.channel === '' || v.channel === chosen.channel),
    );
    return chosen.oldest ? list.reverse() : list;
  }, [versions, chosen.game, chosen.loader, chosen.channel, chosen.oldest]);
  const filtered = chosen.game !== '' || chosen.loader !== '' || chosen.channel !== '';
  const loaderNames = strings.mods.loaderNames as Record<string, string | undefined>;

  const back = (
    <Button onClick={onBack} className={styles.back}>
      {t.back}
    </Button>
  );
  if (project === null) {
    return (
      <div className={styles.section}>
        {back}
        {error !== null ? (
          <Notice tone="danger">{error}</Notice>
        ) : (
          <p role="status">{strings.mods.loading}</p>
        )}
      </div>
    );
  }
  const icon = ourPicture(project.icon);
  // Players' own packs are reported to the catalog's owner; the rest go to their source.
  const own = project.sources.some((s) => s.source === 'own');
  return (
    <article className={styles.section} aria-label={project.title}>
      {back}
      <header className={styles.projectHead}>
        {icon !== null ? (
          <img className={styles.bigIcon} src={icon} alt="" />
        ) : (
          <span className={styles.bigIcon} aria-hidden="true" />
        )}
        <div className={styles.cardBody}>
          <h2 className={styles.projectTitle}>
            {project.title || project.slug}
            {own && <StatusBadge tone="neutral" label={t.playerPack} />}
            {project.status === 'importing' && (
              <StatusBadge tone="warning" label={strings.mods.importing} />
            )}
          </h2>
          {project.authors.length > 0 && (
            <p className={styles.muted}>
              {strings.mods.by({ author: project.authors.map((a) => a.name).join(', ') })}
            </p>
          )}
          <p>{project.summary}</p>
          <p className={styles.meta}>
            {strings.mods.downloads({ count: compact(project.downloads) })}
            {project.updated > 0 &&
              ` · ${strings.mods.updatedOn({ date: dateOf(project.updated) })}`}
            {project.license !== '' && ` · ${t.license({ license: project.license })}`}
          </p>
          {project.categories.length > 0 && (
            <p className={styles.tags}>
              {project.categories.map((c) => (
                <span key={c} className={styles.source}>
                  {c}
                </span>
              ))}
            </p>
          )}
          <div className={forms.actions}>
            {project.sources.map(
              (s) =>
                s.url !== null && (
                  <Button
                    key={s.source}
                    onClick={() => {
                      if (s.url !== null) onLink(s.url);
                    }}
                  >
                    {s.source === 'external'
                      ? t.openSite
                      : t.openOn({ site: strings.mods.sources[s.source] })}
                  </Button>
                ),
            )}
            {own && !reporting && (
              <Button
                onClick={() => {
                  setReporting(true);
                }}
              >
                {t.report}
              </Button>
            )}
          </div>
          {own && reporting && (
            <ReportForm
              projectId={project.id}
              onClose={() => {
                setReporting(false);
              }}
            />
          )}
        </div>
      </header>

      {project.status === 'importing' && <Notice tone="info">{t.importingHint}</Notice>}
      {project.goneAt !== null && <Notice tone="info">{t.gone}</Notice>}
      {project.kind === 'modpack' && <Notice tone="info">{strings.mods.modpacksHint}</Notice>}
      {error !== null && <Notice tone="danger">{error}</Notice>}

      {project.body.length > 0 && (
        <section aria-label={t.description}>
          <h3 className={styles.subTitle}>{t.description}</h3>
          <RichText nodes={project.body} onLink={onLink} />
        </section>
      )}

      {project.gallery.length > 0 && (
        <section aria-label={t.gallery}>
          <h3 className={styles.subTitle}>{t.gallery}</h3>
          <ul className={styles.gallery}>
            {project.gallery.map((picture) => {
              const src = ourPicture(picture.thumb) ?? ourPicture(picture.url);
              return (
                src !== null && (
                  <li key={picture.url}>
                    <figure>
                      <img src={src} alt={picture.title} loading="lazy" />
                      {picture.title !== '' && <figcaption>{picture.title}</figcaption>}
                    </figure>
                  </li>
                )
              );
            })}
          </ul>
        </section>
      )}

      <section aria-label={t.versions}>
        <h3 className={styles.subTitle}>{t.versions}</h3>
        {instance === null && installable(project.kind) && (
          <p className={styles.muted}>{t.chooseInstallation}</p>
        )}
        {versions !== null && versions.length > 1 && (
          <div className={styles.controls}>
            {options.games.length > 1 && (
              <Field label={strings.mods.gameVersion}>
                {(props) => (
                  <select
                    {...props}
                    className={forms.select}
                    value={chosen.game}
                    onChange={(event) => {
                      change({ game: event.target.value });
                    }}
                  >
                    <option value="">{strings.mods.anyVersion}</option>
                    {options.games.map((game) => (
                      <option key={game} value={game}>
                        {game}
                      </option>
                    ))}
                  </select>
                )}
              </Field>
            )}
            {options.loaders.length > 1 && (
              <Field label={strings.mods.loader}>
                {(props) => (
                  <select
                    {...props}
                    className={forms.select}
                    value={chosen.loader}
                    onChange={(event) => {
                      change({ loader: event.target.value });
                    }}
                  >
                    <option value="">{strings.mods.anyLoader}</option>
                    {options.loaders.map((loader) => (
                      <option key={loader} value={loader}>
                        {loaderNames[loader] ?? loader}
                      </option>
                    ))}
                  </select>
                )}
              </Field>
            )}
            <Field label={t.channel}>
              {(props) => (
                <select
                  {...props}
                  className={forms.select}
                  value={chosen.channel}
                  onChange={(event) => {
                    change({ channel: event.target.value as typeof chosen.channel });
                  }}
                >
                  <option value="">{t.anyChannel}</option>
                  {CHANNELS.map((channel) => (
                    <option key={channel} value={channel}>
                      {t.channels[channel]}
                    </option>
                  ))}
                </select>
              )}
            </Field>
            <Field label={t.order}>
              {(props) => (
                <select
                  {...props}
                  className={forms.select}
                  value={chosen.oldest ? 'oldest' : 'newest'}
                  onChange={(event) => {
                    change({ oldest: event.target.value === 'oldest' });
                  }}
                >
                  <option value="newest">{t.newest}</option>
                  <option value="oldest">{t.oldest}</option>
                </select>
              )}
            </Field>
          </div>
        )}
        {versions === null ? (
          <p role="status">{strings.mods.loading}</p>
        ) : versions.length === 0 ? (
          <p className={styles.muted}>{t.noVersions}</p>
        ) : matching.length === 0 ? (
          <p className={styles.muted}>
            {t.noneMatch}{' '}
            <Button
              onClick={() => {
                change({ game: '', loader: '', channel: '' });
              }}
            >
              {t.showAll}
            </Button>
          </p>
        ) : (
          <>
            {filtered && (
              <p className={styles.muted} role="status">
                {t.matching({ count: matching.length, total: versions.length })}
              </p>
            )}
            <ul className={styles.versions}>
              {matching.slice(0, shown).map((version) => (
                <VersionRow
                  key={version.id}
                  version={version}
                  project={project}
                  instance={instance}
                  onLink={onLink}
                  onAdd={() => {
                    onAdd(project, version);
                  }}
                  onInstallPack={() => {
                    onInstallPack(project, version);
                  }}
                />
              ))}
            </ul>
          </>
        )}
        {matching.length > shown && (
          <Button
            onClick={() => {
              setShown(shown + SHOWN);
            }}
          >
            {strings.mods.more}
          </Button>
        )}
      </section>
    </article>
  );
}
