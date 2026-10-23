import { useEffect, useId, useRef, useState } from 'react';
import { Icon } from '../../components/Icon';
import { Notice } from '../../components/Notice';
import type {
  ContentItem,
  ContentView,
  InstanceSummary,
  ModsHit,
  ModsKind,
  ModsProject,
  ModsVersion,
} from '../../lib/contracts';
import { describeIpcError, ipc } from '../../lib/ipc';
import { numberLocale, strings } from '../../lib/strings';
import { cannotAdd, installKind, isolate, loaderOf } from './helpers';
import { InstallationSelect } from './InstallationSelect';
import { type Offer, ProjectCard } from './ProjectCard';
import { ProjectDescription } from './ProjectDescription';
import { ProjectDependencies, ProjectSimilar } from './ProjectRelated';
import { ProjectVersions, type VersionsSummary } from './ProjectVersions';
import {
  bestPack,
  bestVersion,
  dependenciesOf,
  gameRange,
  pageOf,
  versionsFor,
} from './projectVersions';
import catalog from './Catalog.module.css';
import styles from './Project.module.css';

/** Similar projects listed at most. */
const SIMILAR = 5;

/** A project a quick add takes: the newest version that fits, or its page. */
export interface QuickTarget {
  id: string;
  kind: ModsKind;
  title: string;
  slug: string;
}

interface Props {
  /** Our ID or the slug. */
  projectRef: string;
  instance: InstanceSummary | null;
  instances: readonly InstanceSummary[];
  onInstance: (instanceId: string | null) => void;
  /** Opened from the data packs: a mod's data pack versions come first. */
  datapacks?: boolean;
  /** Changes after an install: what the installation holds is read again. */
  revision: number;
  onBack: () => void;
  onLink: (url: string) => void;
  onOpen: (projectId: string) => void;
  onAdd: (project: ModsProject, version: ModsVersion, replaces: string | null) => void;
  onInstallPack: (project: ModsProject, version: ModsVersion) => void;
  onAddToCatalog: (text: string) => void;
  onQuickAdd: (target: QuickTarget) => void;
}

/** An answer kept with the question it answers, so a stale one is never shown. */
type Answer<T> = { for: string; value: T; error?: undefined } | { for: string; error: string };

interface Summary extends VersionsSummary {
  /** The newest version of all. */
  newest: ModsVersion | null;
}

/** Asks once per key; the answer comes back tagged with it. */
function useAnswer<T>(key: string | null, ask: () => Promise<T>): Answer<T> | null {
  const [answer, setAnswer] = useState<Answer<T> | null>(null);
  const askRef = useRef(ask);
  useEffect(() => {
    askRef.current = ask;
  });
  useEffect(() => {
    if (key === null) return;
    let live = true;
    askRef
      .current()
      .then((value) => {
        if (live) setAnswer({ for: key, value });
      })
      .catch((e: unknown) => {
        if (live) setAnswer({ for: key, error: describeIpcError(e) });
      });
    return () => {
      live = false;
    };
  }, [key]);
  return key !== null && answer?.for === key ? answer : null;
}

/**
 * A project's page (D-066), concept A variant 2 "sticky card" (D-072): the card with
 * the main action, what it depends on and similar projects stay at the inline start
 * while the description and the versions scroll; the versions scroll inside their
 * panel.
 */
export function ProjectPage({
  projectRef,
  instance,
  instances,
  onInstance,
  datapacks = false,
  revision,
  onBack,
  onLink,
  onOpen,
  onAdd,
  onInstallPack,
  onAddToCatalog,
  onQuickAdd,
}: Props) {
  const t = strings.mods.project;
  const selectId = useId();
  const select = useRef<HTMLSelectElement>(null);
  const heading = useRef<HTMLHeadingElement>(null);
  const focused = useRef(false);
  const [attempt, setAttempt] = useState(0);
  const [versionsAttempt, setVersionsAttempt] = useState(0);

  const loaded = useAnswer(`${projectRef}:${String(attempt)}`, () =>
    ipc.modsProject(projectRef).then((found) => {
      const value = found as Partial<ModsProject> | null;
      if (value === null || typeof value !== 'object' || typeof value.id !== 'string') {
        throw new Error('project');
      }
      return found;
    }),
  );
  const project = loaded?.error === undefined ? (loaded?.value ?? null) : null;
  const projectId = project?.id ?? null;
  const kind = project?.kind ?? null;

  const summary = useAnswer<Summary>(
    projectId === null ? null : `${projectId}:${String(versionsAttempt)}`,
    () =>
      ipc
        .modsVersionsPage(projectId ?? '', {
          game: null,
          loader: null,
          channel: null,
          offset: 0,
          limit: 1,
        })
        .then((answer) => {
          const found = pageOf(answer);
          return {
            total: found.total,
            games: found.games,
            loaders: found.loaders,
            newest: found.versions[0] ?? null,
          };
        }),
  );
  // The versions made for the installation's game version, which Add picks from; for a
  // modpack, which makes an installation of its own, its newest versions.
  const game = kind === null ? null : kind === 'modpack' ? '' : (instance?.versionId ?? null);
  const candidates = useAnswer(
    projectId === null || game === null ? null : `${projectId}:${game}:${String(versionsAttempt)}`,
    () => versionsFor(projectId ?? '', game === '' ? null : game, game === '' ? 1 : undefined),
  );
  const instanceId = instance?.id ?? null;
  const content = useAnswer<ContentItem[]>(
    instanceId === null ? null : `${instanceId}:${String(revision)}`,
    () =>
      ipc.modsContent(instanceId ?? '').then((view) => {
        const items = (view as Partial<ContentView> | null)?.items;
        return Array.isArray(items) ? items : [];
      }),
  );
  const similarLoader = kind === null ? '' : loaderOf(kind, instance);
  const similar = useAnswer<ModsHit[]>(
    projectId === null
      ? null
      : JSON.stringify([projectId, instance?.versionId ?? null, similarLoader]),
    () =>
      ipc
        .modsSimilar(projectId ?? '', instance?.versionId ?? null, similarLoader || null)
        .then((hits) =>
          Array.isArray(hits) ? hits.filter((hit) => hit.id !== projectId).slice(0, SIMILAR) : [],
        ),
  );
  const rows = candidates?.error === undefined ? (candidates?.value ?? null) : null;
  const items = content?.error === undefined ? (content?.value ?? null) : null;
  // This project in the installation, outside the worlds.
  const mine =
    projectId === null
      ? null
      : (items?.find((item) => item.projectId === projectId && item.world === null) ?? null);
  // The version it holds when Add's list has not got it (one made for another game
  // version): its date decides whether Add's is newer, as the core decides updates.
  const unlisted =
    mine !== null &&
    mine.versionId !== null &&
    rows !== null &&
    !rows.rows.some((version) => version.id === mine.versionId)
      ? mine.versionId
      : null;
  const held = useAnswer<ModsVersion>(unlisted, () =>
    ipc.modsVersion(unlisted ?? '').then((found) => {
      const value = found as Partial<ModsVersion> | null;
      if (
        value === null ||
        typeof value !== 'object' ||
        value.id !== unlisted ||
        typeof value.published !== 'number' ||
        !Array.isArray(value.dependencies)
      ) {
        throw new Error('version');
      }
      return found;
    }),
  );

  // The tile pressed is gone: the focus starts on the project's name.
  useEffect(() => {
    if (focused.current || heading.current === null) return;
    focused.current = true;
    heading.current.focus({ preventScroll: true });
  });

  const back = (
    <header className={styles.head}>
      <button type="button" className={styles.back} onClick={onBack}>
        <Icon name="back" size={18} className={catalog.mirror} />
        {t.backToResults}
      </button>
      <span className={catalog.spacer} />
      <InstallationSelect
        id={selectId}
        instances={instances}
        value={instance?.id ?? null}
        onChange={onInstance}
        selectRef={select}
      />
    </header>
  );

  if (project === null || kind === null) {
    return (
      <div className={styles.page}>
        {back}
        {loaded?.error !== undefined ? (
          <div className={catalog.empty}>
            <p className={catalog.emptyTitle}>{loaded.error}</p>
            <button
              type="button"
              className={[catalog.smallButton, catalog.soft].join(' ')}
              onClick={() => {
                setAttempt((n) => n + 1);
              }}
            >
              {strings.mods.catalog.tryAgain}
            </button>
          </div>
        ) : (
          <div className={styles.grid} aria-busy="true">
            <p role="status" className={catalog.visuallyHidden}>
              {strings.mods.loading}
            </p>
            <div className={[catalog.skeleton, styles.skeletonCard].join(' ')} aria-hidden="true">
              <span className={catalog.skeletonIcon} />
              <span className={catalog.skeletonLine} />
              <span className={[catalog.skeletonLine, catalog.skeletonShort].join(' ')} />
            </div>
            <div className={[catalog.skeleton, styles.skeletonCard].join(' ')} aria-hidden="true">
              <span className={catalog.skeletonLine} />
              <span className={catalog.skeletonLine} />
              <span className={[catalog.skeletonLine, catalog.skeletonShort].join(' ')} />
            </div>
          </div>
        )}
      </div>
    );
  }

  const versionsError = summary?.error ?? candidates?.error ?? null;
  const facts = summary?.error === undefined ? (summary?.value ?? null) : null;
  const installedIds =
    items === null
      ? null
      : new Set(items.flatMap((item) => (item.projectId === null ? [] : [item.projectId])));

  // The main action, and the version it is about: the one it takes, or the one installed.
  const [offer, about] = ((): [Offer, ModsVersion | null] => {
    if (kind === 'modpack') {
      if (rows === null) {
        return versionsError === null
          ? [{ state: 'loading' }, null]
          : [{ state: 'pack', version: undefined, reason: t.loadFailed }, null];
      }
      const pack = bestPack(rows.rows);
      return [{ state: 'pack', version: pack }, pack ?? null];
    }
    if (instance === null) return [{ state: 'choose' }, null];
    if (rows === null) {
      return versionsError === null
        ? [{ state: 'loading' }, null]
        : [{ state: 'none', reason: t.loadFailed }, null];
    }
    const best = bestVersion(kind, rows.rows, instance, datapacks);
    if (best === undefined) {
      const near = rows.rows[0] ?? facts?.newest ?? undefined;
      const reason = near === undefined ? null : cannotAdd(installKind(kind, near), near, instance);
      return [
        { state: 'none', reason: reason ?? t.noneFor({ game: isolate(instance.versionId) }) },
        null,
      ];
    }
    if (mine !== null && installKind(kind, best) !== 'datapack') {
      if (mine.versionId === best.id) return [{ state: 'installed', number: best.number }, best];
      const had =
        rows.rows.find((version) => version.id === mine.versionId) ??
        (held?.error === undefined ? held?.value : undefined);
      if (had === undefined && unlisted !== null) {
        // Still asking; or it could not be read, and what is there stays, as the core
        // leaves it.
        return held === null
          ? [{ state: 'loading' }, null]
          : [{ state: 'installed', number: mine.versionNumber ?? mine.name }, null];
      }
      if (had !== undefined && had.published >= best.published) {
        return [{ state: 'installed', number: had.number }, had];
      }
      return [{ state: 'update', version: best, replaces: mine.path }, best];
    }
    return [{ state: 'add', version: best }, best];
  })();
  const offered =
    offer.state === 'add' || offer.state === 'update' || offer.state === 'pack'
      ? (offer.version ?? null)
      : null;
  const offeredFile = offered?.files.find((file) => file.primary) ?? offered?.files[0];

  const range = facts === null ? null : gameRange(facts.games);
  const count = facts?.total.toLocaleString(numberLocale()) ?? '';
  const versionsFact =
    facts === null || facts.total === 0
      ? null
      : range === null
        ? count
        : range.from === range.to
          ? t.versionsFor({ count, game: isolate(range.from) })
          : t.versionsRange({ count, from: isolate(range.from), to: isolate(range.to) });

  // What the version the card is about depends on; without one, the newest made for the
  // game.
  const depsOf = about ?? (kind === 'modpack' ? null : (rows?.rows[0] ?? facts?.newest ?? null));
  const dependencies = depsOf === null ? [] : dependenciesOf(depsOf);
  const similarHits = similar?.error === undefined ? (similar?.value ?? []) : [];
  const title = project.title || project.slug;

  return (
    <div className={styles.page}>
      {back}
      <article className={styles.grid} aria-label={title}>
        <div className={styles.side}>
          <ProjectCard
            project={project}
            instance={instance}
            offer={offer}
            versions={versionsFact}
            checked={offeredFile?.checked === true}
            headingRef={heading}
            onAdd={(version, replaces) => {
              onAdd(project, version, replaces);
            }}
            onInstallPack={(version) => {
              onInstallPack(project, version);
            }}
            onLink={onLink}
            onChooseInstallation={() => {
              select.current?.focus();
            }}
          />
          {depsOf !== null && dependencies.length > 0 && (
            <ProjectDependencies
              rows={dependencies}
              forVersion={depsOf === offered ? null : depsOf.number}
              installed={installedIds}
              canAdd={instance !== null}
              comes={depsOf === offered && (offer.state === 'add' || offer.state === 'update')}
              onOpen={onOpen}
              onQuickAdd={(target) => {
                onQuickAdd({ ...target, kind: 'mod' });
              }}
              onAddToCatalog={onAddToCatalog}
            />
          )}
          {similarHits.length > 0 && <ProjectSimilar hits={similarHits} onOpen={onOpen} />}
        </div>
        <div className={styles.main}>
          {project.status === 'importing' && <Notice tone="info">{t.importingHint}</Notice>}
          {project.goneAt !== null && <Notice tone="info">{t.gone}</Notice>}
          {kind === 'modpack' && <Notice tone="info">{strings.mods.modpacksHint}</Notice>}
          <ProjectDescription project={project} onLink={onLink} />
          {versionsError !== null ? (
            <section className={styles.panel} aria-label={t.versions}>
              <h3 className={styles.panelTitle}>{t.versions}</h3>
              <div className={catalog.empty}>
                <p className={catalog.emptyTitle}>{t.loadFailed}</p>
                <p className={styles.hint}>{versionsError}</p>
                <button
                  type="button"
                  className={[catalog.smallButton, catalog.soft].join(' ')}
                  onClick={() => {
                    setVersionsAttempt((n) => n + 1);
                  }}
                >
                  {strings.mods.catalog.tryAgain}
                </button>
              </div>
            </section>
          ) : facts === null ? (
            <section className={styles.panel} aria-label={t.versions} aria-busy="true">
              <h3 className={styles.panelTitle}>{t.versions}</h3>
              <p role="status" className={styles.hint}>
                {strings.mods.loading}
              </p>
            </section>
          ) : (
            <ProjectVersions
              key={versionsAttempt}
              project={project}
              instance={instance}
              summary={facts}
              fit={kind === 'modpack' || instance === null ? null : (rows ?? 'loading')}
              datapacks={datapacks}
              installedVersion={mine?.versionId ?? null}
              onAdd={(version) => {
                // Another version of it in the installation is replaced.
                const replaces =
                  mine !== null && installKind(kind, version) !== 'datapack' ? mine.path : null;
                onAdd(project, version, replaces);
              }}
              onInstallPack={(version) => {
                onInstallPack(project, version);
              }}
              onLink={onLink}
            />
          )}
        </div>
      </article>
    </div>
  );
}
