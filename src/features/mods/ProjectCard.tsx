import { type Ref, useId, useState } from 'react';
import { Button } from '../../components/Button';
import { Icon } from '../../components/Icon';
import { Notice } from '../../components/Notice';
import type { InstanceSummary, ModsProject, ModsVersion } from '../../lib/contracts';
import { describeIpcError, ipc } from '../../lib/ipc';
import { strings } from '../../lib/strings';
import { categoryName, categoryOf } from './catalogCategories';
import { ProjectIcon } from './CatalogTile';
import { compact, isolate } from './helpers';
import { dayOf } from './versions';
import { ReportForm } from './ReportForm';
import catalog from './Catalog.module.css';
import styles from './Project.module.css';

/** Authors named on the card; the rest are counted. */
const AUTHORS = 3;
/** Categories shown as chips. */
const CATEGORIES = 3;

/** What the card's main button does, worked out from the versions and the installation. */
export type Offer =
  | { state: 'loading' }
  | { state: 'choose' }
  | { state: 'none'; reason: string }
  | { state: 'add'; version: ModsVersion }
  | { state: 'update'; version: ModsVersion; replaces: string }
  | { state: 'installed'; number: string }
  | { state: 'pack'; version: ModsVersion | undefined; reason?: string };

interface Props {
  project: ModsProject;
  instance: InstanceSummary | null;
  offer: Offer;
  /** "61, from 1.16 to 26.3"; null while unknown. */
  versions: string | null;
  /** The catalog keeps the version Add takes and found it clean. */
  checked: boolean;
  headingRef: Ref<HTMLHeadingElement>;
  onAdd: (version: ModsVersion, replaces: string | null) => void;
  onInstallPack: (version: ModsVersion) => void;
  onLink: (url: string) => void;
  onChooseInstallation: () => void;
}

/** The hint under the main button: which version it is, or why there is none. */
function hintOf(offer: Offer, instance: InstanceSummary | null, kind: ModsProject['kind']) {
  const t = strings.mods.project;
  const names = strings.mods.loaderNames as Record<string, string | undefined>;
  switch (offer.state) {
    case 'loading':
      return '';
    case 'choose':
      return t.chooseInstallation;
    case 'none':
      return offer.reason;
    case 'installed':
      return t.installedIn({
        name: isolate(instance?.displayName ?? ''),
        number: isolate(offer.number),
      });
    case 'pack':
      return offer.version === undefined
        ? (offer.reason ?? t.noVersions)
        : t.latestPack({ date: dayOf(offer.version.published) });
    case 'add':
    case 'update': {
      const game = isolate(instance?.versionId ?? '');
      const date = dayOf(offer.version.published);
      const loader = instance?.loader?.kind;
      return kind === 'mod' && loader !== undefined
        ? t.latest({ game, loader: names[loader] ?? loader, date })
        : t.latestGame({ game, date });
    }
  }
}

/**
 * The project's card at the inline start (D-072, variant 2 "sticky card"): its icon,
 * name and authors, kind and categories, the one main action with what it takes,
 * its sites and its facts.
 */
export function ProjectCard({
  project,
  instance,
  offer,
  versions,
  checked,
  headingRef,
  onAdd,
  onInstallPack,
  onLink,
  onChooseInstallation,
}: Props) {
  const t = strings.mods.project;
  const headingId = useId();
  const hintId = useId();
  const [reporting, setReporting] = useState(false);
  // The version the player asked the catalog to keep, and what went wrong.
  const [asked, setAsked] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const title = project.title || project.slug;
  const own = project.sources.some((s) => s.source === 'own');
  const authors = project.authors.map((a) => a.name).filter((name) => name !== '');
  const shownAuthors = authors.slice(0, AUTHORS).join(', ');
  const categories = project.categoryIds
    .map((id) => categoryOf(project.kind, id))
    .filter((category) => category !== null)
    .slice(0, CATEGORIES);
  const links = project.sources.filter(
    (s): s is { source: ModsProject['sources'][number]['source']; url: string } =>
      s.url !== null && s.source !== 'own',
  );
  const sources = project.sources
    .filter((s) => s.source !== 'external')
    .map((s) => strings.mods.sources[s.source])
    .join(', ');
  const hint = hintOf(offer, instance, project.kind);
  const version = offer.state === 'add' || offer.state === 'update' ? offer.version : null;
  const keep = version !== null && version.saved !== 'all';

  return (
    <section className={[styles.panel, styles.card].join(' ')} aria-labelledby={headingId}>
      <div className={styles.cardHead}>
        <ProjectIcon hit={project} className={styles.bigIcon} />
        <div className={styles.cardTitle}>
          <h2 id={headingId} ref={headingRef} tabIndex={-1} className={styles.name} dir="auto">
            {title}
          </h2>
          {authors.length > 0 && (
            <p className={styles.hint} dir="auto">
              {authors.length > AUTHORS
                ? strings.mods.catalog.andMore({
                    names: isolate(shownAuthors),
                    count: authors.length - AUTHORS,
                  })
                : shownAuthors}
            </p>
          )}
        </div>
      </div>

      <ul className={styles.chips}>
        <li className={catalog.chip}>{t.kinds[project.kind]}</li>
        {own && <li className={catalog.chip}>{t.playerPack}</li>}
        {project.status === 'importing' && (
          <li className={[catalog.chip, styles.chipWarning].join(' ')}>{strings.mods.importing}</li>
        )}
        {categories.map((category) => (
          <li key={category.id} className={styles.outlineChip}>
            {categoryName(category.id)}
          </li>
        ))}
      </ul>

      <div className={styles.action}>
        {offer.state === 'loading' ? (
          <span className={[catalog.skeletonLine, styles.mainSkeleton].join(' ')} />
        ) : offer.state === 'choose' ? (
          <Button
            className={styles.mainAction}
            aria-describedby={hintId}
            onClick={onChooseInstallation}
          >
            {t.chooseButton}
          </Button>
        ) : offer.state === 'installed' ? null : offer.state === 'pack' ? (
          <span title={offer.version === undefined ? (offer.reason ?? t.noVersions) : undefined}>
            <Button
              variant="primary"
              className={styles.mainAction}
              disabled={offer.version === undefined}
              aria-describedby={hintId}
              onClick={() => {
                if (offer.version !== undefined) onInstallPack(offer.version);
              }}
            >
              <Icon name="downloads" size={18} />
              {offer.version === undefined
                ? strings.mods.pack.install
                : t.installVersion({ number: isolate(offer.version.number) })}
            </Button>
          </span>
        ) : offer.state === 'none' ? (
          // A disabled button gets no pointer events everywhere: its wrapper carries the
          // reason too, and the hint under it says it in words.
          <span title={offer.reason}>
            <Button
              variant="primary"
              className={styles.mainAction}
              disabled
              title={offer.reason}
              aria-describedby={hintId}
            >
              <Icon name="plus" size={18} />
              {t.add}
            </Button>
          </span>
        ) : (
          <Button
            variant="primary"
            className={styles.mainAction}
            aria-describedby={hintId}
            onClick={() => {
              onAdd(offer.version, offer.state === 'update' ? offer.replaces : null);
            }}
          >
            <Icon name={offer.state === 'update' ? 'update' : 'plus'} size={18} />
            {offer.state === 'update'
              ? t.updateTo({ number: isolate(offer.version.number) })
              : t.addVersion({ number: isolate(offer.version.number) })}
          </Button>
        )}
        {hint !== '' && (
          <p
            id={hintId}
            className={[styles.hint, offer.state === 'installed' && styles.installed]
              .filter(Boolean)
              .join(' ')}
          >
            {offer.state === 'installed' && <Icon name="check" size={18} />}
            {hint}
          </p>
        )}
        {keep && (
          <div className={styles.keep}>
            <p className={styles.hint}>{asked === version.id ? t.fetchAsked : t.notKept}</p>
            {asked !== version.id && (
              <button
                type="button"
                className={[catalog.smallButton, styles.ghostButton].join(' ')}
                onClick={() => {
                  setError(null);
                  const id = version.id;
                  ipc
                    .modsFetchVersion(id)
                    .then(() => {
                      setAsked(id);
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
        )}
        {error !== null && <Notice tone="danger">{error}</Notice>}
      </div>

      {(links.length > 0 || own) && (
        <div className={styles.links}>
          {links.map((s) => (
            <button
              key={s.source}
              type="button"
              className={[catalog.smallButton, styles.ghostButton, styles.link].join(' ')}
              aria-label={
                s.source === 'external'
                  ? t.openSite
                  : t.openOn({ site: strings.mods.sources[s.source] })
              }
              onClick={() => {
                onLink(s.url);
              }}
            >
              <Icon name="link" size={18} />
              {s.source === 'external' ? t.authorSite : strings.mods.sources[s.source]}
            </button>
          ))}
          {own && !reporting && (
            <button
              type="button"
              className={[catalog.smallButton, styles.quiet].join(' ')}
              onClick={() => {
                setReporting(true);
              }}
            >
              <Icon name="warn" size={18} />
              {t.report}
            </button>
          )}
        </div>
      )}
      {own && reporting && (
        <ReportForm
          projectId={project.id}
          onClose={() => {
            setReporting(false);
          }}
        />
      )}

      <dl className={styles.facts}>
        <dt>{t.facts.downloads}</dt>
        <dd>{compact(project.downloads)}</dd>
        {project.license !== '' && (
          <>
            <dt>{t.facts.license}</dt>
            <dd dir="auto">{project.license}</dd>
          </>
        )}
        {sources !== '' && (
          <>
            <dt>{t.facts.source}</dt>
            <dd>{checked ? t.sourceChecked({ sources }) : sources}</dd>
          </>
        )}
        {versions !== null && (
          <>
            <dt>{t.facts.versions}</dt>
            <dd>{versions}</dd>
          </>
        )}
      </dl>
    </section>
  );
}
