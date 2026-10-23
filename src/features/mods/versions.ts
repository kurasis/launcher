import type { InstanceSummary, ModsKind, ModsVersion, ModsVersionPage } from '../../lib/contracts';
import { ipc } from '../../lib/ipc';
import { numberLocale } from '../../lib/strings';
import { cannotAdd, installKind, packArchive } from './helpers';

/** Rows of one request for versions: the core takes at most this many. */
export const PAGE_LIMIT = 100;

/** Pages of versions made for the installation's game version read at most. */
const FIT_PAGES = 5;

/**
 * A version made for the installation: it goes in, and it is made for the
 * installation's game version.
 */
export function fits(
  kind: ModsKind,
  version: ModsVersion,
  instance: InstanceSummary | null,
): boolean {
  return (
    instance !== null &&
    version.games.includes(instance.versionId) &&
    cannotAdd(installKind(kind, version), version, instance) === null
  );
}

/**
 * The version Add takes: the newest release made for the installation, else its
 * newest version of another type. Opened from the data packs, a mod's data pack
 * versions come first; `strict` takes no other (the tiles' quick add), otherwise
 * the rest are next.
 */
export function bestVersion(
  kind: ModsKind,
  versions: readonly ModsVersion[],
  instance: InstanceSummary | null,
  datapacks: boolean,
  strict = false,
): ModsVersion | undefined {
  const wantPack = datapacks || kind === 'datapack';
  const fitting = versions.filter((version) => fits(kind, version, instance));
  const preferred = fitting.filter(
    (version) => (installKind(kind, version) === 'datapack') === wantPack,
  );
  const pick = (list: ModsVersion[]) =>
    list.find((version) => version.channel === 'release') ?? list[0];
  return pick(preferred) ?? (strict ? undefined : pick(fitting));
}

/** A modpack's version to install: the newest release with an archive the catalog lets out. */
export function bestPack(versions: readonly ModsVersion[]): ModsVersion | undefined {
  const ready = versions.filter(
    (version) =>
      version.files.some((file) => packArchive(file.name)) &&
      !version.files.some((file) => file.held),
  );
  return ready.find((version) => version.channel === 'release') ?? ready[0];
}

/** The oldest and newest releases of the game among these, for "from … to …". */
export function gameRange(games: readonly string[]): { from: string; to: string } | null {
  const releases = games
    .filter((game) => /^\d+(\.\d+)+$/.test(game))
    .sort((a, b) => a.localeCompare(b, 'en', { numeric: true }));
  const from = releases[0];
  const to = releases.at(-1);
  return from === undefined || to === undefined ? null : { from, to };
}

function dayFormat(seconds: number, month: 'long' | 'short', now: number): string {
  if (!Number.isFinite(seconds) || seconds <= 0) return '';
  const date = new Date(seconds * 1000);
  // Past the dates a Date can hold: no date rather than a throw.
  if (Number.isNaN(date.getTime())) return '';
  const thisYear = date.getFullYear() === new Date(now).getFullYear();
  return new Intl.DateTimeFormat(numberLocale(), {
    day: 'numeric',
    month,
    ...(thisYear ? {} : { year: 'numeric' }),
  }).format(date);
}

/** "12 October", with the year when it is another one. */
export function dayOf(seconds: number, now = Date.now()): string {
  return dayFormat(seconds, 'long', now);
}

/** "12 Oct", for the versions table. */
export function shortDayOf(seconds: number, now = Date.now()): string {
  return dayFormat(seconds, 'short', now);
}

/** An answer about versions, checked for its shape: the rows and how many match. */
export function pageOf(answer: unknown): ModsVersionPage {
  const found = answer as Partial<ModsVersionPage> | null;
  if (found === null || typeof found !== 'object' || !Array.isArray(found.versions)) {
    throw new Error('versions');
  }
  return {
    total: typeof found.total === 'number' ? found.total : found.versions.length,
    versions: found.versions,
    games: Array.isArray(found.games) ? found.games : [],
    loaders: Array.isArray(found.loaders) ? found.loaders : [],
  };
}

/**
 * The versions made for a game version (of every loader and type), newest first:
 * page after page, up to five. `capped` when there are more.
 */
export async function versionsFor(
  projectId: string,
  game: string | null,
  pages = FIT_PAGES,
): Promise<{ rows: ModsVersion[]; capped: boolean }> {
  const rows: ModsVersion[] = [];
  for (let page = 0; page < pages; page += 1) {
    const found = pageOf(
      await ipc.modsVersionsPage(projectId, {
        game,
        loader: null,
        channel: null,
        offset: rows.length,
        limit: PAGE_LIMIT,
      }),
    );
    rows.push(...found.versions.slice(0, PAGE_LIMIT));
    if (found.versions.length < PAGE_LIMIT || rows.length >= found.total) {
      return { rows, capped: false };
    }
  }
  return { rows, capped: true };
}

type Dependency = ModsVersion['dependencies'][number];

/** The kinds shown, in this order: what is embedded comes inside the file. */
const ORDER: Dependency['kind'][] = ['required', 'optional', 'incompatible'];

/** A Modrinth page the core gave for a project the catalog has not got. */
export function modrinthPage(url: string | null): string | null {
  return url !== null && url.length <= 200 && /^https:\/\/modrinth\.com\/[\w./-]+$/.test(url)
    ? url
    : null;
}

/** One row per project, in the order above; ones with nothing to name them by are left out. */
export function dependenciesOf(version: ModsVersion): Dependency[] {
  const seen = new Set<string>();
  const rows: Dependency[] = [];
  for (const kind of ORDER) {
    for (const dependency of version.dependencies) {
      if (dependency.kind !== kind) continue;
      const key =
        dependency.projectId ??
        modrinthPage(dependency.sourcePage) ??
        (dependency.title || dependency.slug);
      if (key === null || key === '' || seen.has(key)) continue;
      seen.add(key);
      rows.push(dependency);
    }
  }
  return rows;
}
