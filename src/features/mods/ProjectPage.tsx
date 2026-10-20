import { useEffect, useState } from 'react';
import { Button } from '../../components/Button';
import { Notice } from '../../components/Notice';
import { StatusBadge } from '../../components/StatusBadge';
import forms from '../../components/forms.module.css';
import type { InstanceSummary, ModsProject, ModsVersion, DocNode } from '../../lib/contracts';
import { describeIpcError, ipc } from '../../lib/ipc';
import { strings } from '../../lib/strings';
import { compact, dateOf, installable, loaderOf, ourPicture } from './helpers';
import { RichText } from './RichText';
import styles from './Mods.module.css';

const SHOWN = 30;

interface Props {
  /** Our ID or the slug. */
  projectRef: string;
  instance: InstanceSummary | null;
  onBack: () => void;
  onLink: (url: string) => void;
  onAdd: (project: ModsProject, version: ModsVersion) => void;
  onInstallPack: (project: ModsProject, version: ModsVersion) => void;
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
  // A Modrinth pack; other packs come later.
  const pack = project.kind === 'modpack' && version.files.some((f) => f.name.endsWith('.mrpack'));
  return (
    <li className={styles.version}>
      <div className={styles.versionHead}>
        <span className={styles.versionName}>
          <bdi>{version.name || version.number}</bdi>
          {version.channel !== 'release' && (
            <StatusBadge tone="warning" label={t.channels[version.channel]} />
          )}
        </span>
        <span className={styles.muted}>
          <bdi dir="ltr">{version.games.slice(0, 6).join(', ')}</bdi>
          {version.games.length > 6 && ` ${t.andMore({ count: version.games.length - 6 })}`}
          {version.loaders.length > 0 && ` · ${version.loaders.join(', ')}`}
          {` · ${dateOf(version.published)}`}
          {file !== undefined && ` · ${file.name}`}
        </span>
      </div>
      {version.saved !== 'all' && <p className={styles.note}>{asked ? t.fetchAsked : t.notKept}</p>}
      {version.goneAt !== null && <p className={styles.note}>{t.versionGone}</p>}
      {error !== null && <Notice tone="danger">{error}</Notice>}
      <div className={forms.actions}>
        {installable(project.kind) && (
          <Button variant="primary" disabled={instance === null} onClick={onAdd}>
            {t.add}
          </Button>
        )}
        {pack && (
          <Button variant="primary" onClick={onInstallPack}>
            {strings.mods.pack.install}
          </Button>
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
export function ProjectPage({ projectRef, instance, onBack, onLink, onAdd, onInstallPack }: Props) {
  const t = strings.mods.project;
  const [project, setProject] = useState<ModsProject | null>(null);
  // Versions with the query they answer; another query shows none yet.
  const [answer, setAnswer] = useState<{ query: string; list: ModsVersion[] } | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [fitting, setFitting] = useState(instance !== null);
  const [shown, setShown] = useState(SHOWN);

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
  const kind = project?.kind ?? null;
  // A new snapshot is a new object; only these matter here. A modpack makes
  // an installation of its own, so nothing is filtered for one.
  const filtered = fitting && kind !== 'modpack';
  const game = filtered ? (instance?.versionId ?? null) : null;
  const loader = fitting && kind !== null ? loaderOf(kind, instance) : '';
  const query = `${projectId ?? ''}|${game ?? ''}|${loader}`;
  const versions = answer?.query === query ? answer.list : null;
  useEffect(() => {
    if (projectId === null) return;
    let live = true;
    void ipc
      .modsVersions(projectId, game, loader === '' ? null : loader)
      .then((found) => {
        if (live) setAnswer({ query: `${projectId}|${game ?? ''}|${loader}`, list: found });
      })
      .catch((e: unknown) => {
        if (live) setError(describeIpcError(e));
      });
    return () => {
      live = false;
    };
  }, [projectId, game, loader]);

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
                    {t.openOn({ site: strings.mods.sources[s.source] })}
                  </Button>
                ),
            )}
          </div>
        </div>
      </header>

      {project.status === 'importing' && <Notice tone="info">{t.importingHint}</Notice>}
      {project.goneAt !== null && <Notice tone="info">{t.gone}</Notice>}
      {project.kind === 'modpack' && <Notice tone="info">{strings.mods.modpacksHint}</Notice>}
      {project.kind === 'world' && <Notice tone="info">{t.worldsLater}</Notice>}
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
        {instance !== null && project.kind !== 'modpack' && (
          <label className={styles.check}>
            <input
              type="checkbox"
              checked={fitting}
              onChange={(event) => {
                setFitting(event.target.checked);
                setShown(SHOWN);
              }}
            />
            {t.fitsOnly({ name: instance.displayName })}
          </label>
        )}
        {instance === null && installable(project.kind) && (
          <p className={styles.muted}>{t.chooseInstallation}</p>
        )}
        {versions === null ? (
          <p role="status">{strings.mods.loading}</p>
        ) : versions.length === 0 ? (
          <p className={styles.muted}>{filtered ? t.noneFits : t.noVersions}</p>
        ) : (
          <ul className={styles.versions}>
            {versions.slice(0, shown).map((version) => (
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
        )}
        {versions !== null && versions.length > shown && (
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
