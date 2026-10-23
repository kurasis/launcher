import { useId } from 'react';
import { Icon } from '../../components/Icon';
import type { ModsHit, ModsVersion } from '../../lib/contracts';
import { strings } from '../../lib/strings';
import { ProjectIcon } from './CatalogTile';
import { isolate } from './helpers';
import { modrinthPage } from './projectVersions';
import catalog from './Catalog.module.css';
import styles from './Project.module.css';

type Dependency = ModsVersion['dependencies'][number];

/** Dependencies listed; the rest are counted. */
const DEPENDENCIES = 10;

interface DependenciesProps {
  rows: Dependency[];
  /** The version they are of, named when it is not the one Add takes. */
  forVersion: string | null;
  /** Projects the installation holds; null when unknown. */
  installed: ReadonlySet<string> | null;
  /** An installation is chosen: optional ones can be added. */
  canAdd: boolean;
  onOpen: (projectId: string) => void;
  onQuickAdd: (project: { id: string; title: string; slug: string }) => void;
  onAddToCatalog: (url: string) => void;
}

/** What the version needs, may use, and does not work with (D-072). */
export function ProjectDependencies({
  rows,
  forVersion,
  installed,
  canAdd,
  onOpen,
  onQuickAdd,
  onAddToCatalog,
}: DependenciesProps) {
  const t = strings.mods.project.deps;
  const headingId = useId();
  const shown = rows.slice(0, DEPENDENCIES);
  return (
    <section className={[styles.panel, styles.related].join(' ')} aria-labelledby={headingId}>
      <div className={styles.panelHead}>
        <h3 id={headingId} className={styles.panelTitle}>
          {t.title}
        </h3>
        {forVersion !== null && (
          <span className={styles.hint}>{t.forVersion({ number: isolate(forVersion) })}</span>
        )}
      </div>
      <ul className={styles.items}>
        {shown.map((dependency) => {
          const name = dependency.title || dependency.slug || t.unknown;
          const id = dependency.projectId;
          const has = id !== null && installed?.has(id) === true;
          const page = id === null ? modrinthPage(dependency.sourcePage) : null;
          const meta =
            dependency.kind === 'optional'
              ? t.optional
              : dependency.kind === 'incompatible'
                ? t.incompatible
                : canAdd && !has
                  ? t.requiredComes
                  : t.required;
          return (
            <li key={id ?? page ?? name} className={styles.item}>
              <ProjectIcon
                hit={{ icon: dependency.icon, title: name, slug: dependency.slug ?? '' }}
                className={styles.smallIcon}
              />
              <div className={styles.itemText}>
                {id !== null ? (
                  <button
                    type="button"
                    className={styles.itemName}
                    dir="auto"
                    onClick={() => {
                      onOpen(id);
                    }}
                  >
                    {name}
                  </button>
                ) : (
                  <span className={styles.itemName} dir="auto">
                    {name}
                  </span>
                )}
                <span
                  className={[styles.hint, dependency.kind === 'incompatible' && styles.warningText]
                    .filter(Boolean)
                    .join(' ')}
                >
                  {meta}
                </span>
              </div>
              {has ? (
                <span
                  className={[
                    catalog.chip,
                    dependency.kind === 'incompatible' ? styles.chipWarning : catalog.chipSuccess,
                  ].join(' ')}
                >
                  {t.inInstallation}
                </span>
              ) : dependency.kind === 'optional' && id !== null && canAdd ? (
                <button
                  type="button"
                  className={[catalog.smallButton, styles.ghostButton].join(' ')}
                  aria-label={t.addNamed({ title: isolate(name) })}
                  onClick={() => {
                    onQuickAdd({ id, title: name, slug: dependency.slug ?? '' });
                  }}
                >
                  <Icon name="plus" size={18} />
                  {t.add}
                </button>
              ) : page !== null ? (
                <button
                  type="button"
                  className={[catalog.smallButton, styles.ghostButton].join(' ')}
                  aria-label={t.toCatalogNamed({ title: isolate(name) })}
                  onClick={() => {
                    onAddToCatalog(page);
                  }}
                >
                  <Icon name="plus" size={18} />
                  {t.toCatalog}
                </button>
              ) : null}
            </li>
          );
        })}
      </ul>
      {rows.length > shown.length && (
        <p className={styles.hint}>{t.more({ count: rows.length - shown.length })}</p>
      )}
    </section>
  );
}

/** Projects like this one, for the same game version and loader (D-072). */
export function ProjectSimilar({
  hits,
  onOpen,
}: {
  hits: ModsHit[];
  onOpen: (projectId: string) => void;
}) {
  const t = strings.mods.project.similar;
  const headingId = useId();
  return (
    <section className={[styles.panel, styles.related].join(' ')} aria-labelledby={headingId}>
      <h3 id={headingId} className={styles.panelTitle}>
        {t.title}
      </h3>
      <ul className={styles.items}>
        {hits.map((hit) => {
          const name = hit.title || hit.slug;
          return (
            <li key={hit.id} className={styles.item}>
              <ProjectIcon hit={hit} className={styles.smallIcon} />
              <div className={styles.itemText}>
                <span className={styles.itemName} dir="auto">
                  {name}
                </span>
                {hit.summary !== '' && (
                  <span className={[styles.hint, styles.clamp].join(' ')} dir="auto">
                    {hit.summary}
                  </span>
                )}
              </div>
              <button
                type="button"
                className={[catalog.smallButton, styles.ghostButton].join(' ')}
                aria-label={t.openNamed({ title: isolate(name) })}
                onClick={() => {
                  onOpen(hit.id);
                }}
              >
                {t.open}
              </button>
            </li>
          );
        })}
      </ul>
    </section>
  );
}
