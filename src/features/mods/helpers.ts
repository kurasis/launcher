import type { StatusTone } from '../../components/StatusBadge';
import type {
  InstanceSummary,
  LoaderKind,
  ModsImportRequest,
  ModsKind,
  ModsSetProblem,
  ModsVersion,
} from '../../lib/contracts';
import { numberLocale, strings } from '../../lib/strings';

/** Where pictures of the catalog come from; nothing else is drawn (D-066). */
export const PICTURE_ORIGIN = 'https://files-mods.lapi.sh/';

/** A picture address the interface may draw. */
export function ourPicture(url: string | null | undefined): string | null {
  return typeof url === 'string' && url.startsWith(PICTURE_ORIGIN) ? url : null;
}

/** `12K`, `3.4M` in the interface language. */
export function compact(count: number): string {
  return new Intl.NumberFormat(numberLocale(), {
    notation: 'compact',
    maximumFractionDigits: 1,
  }).format(count);
}

/** A date from Unix seconds. */
export function dateOf(seconds: number): string {
  return new Date(seconds * 1000).toLocaleDateString(numberLocale());
}

/** The rule the core checks a game version by (`game_version` in src-tauri/src/mods/mod.rs); empty is any. */
export function validGame(text: string): boolean {
  return text.length <= 40 && /^[A-Za-z0-9._+ -]*$/.test(text);
}

/** The loaders a kind of project is filtered by. */
export function loadersFor(kind: ModsKind): readonly string[] {
  switch (kind) {
    case 'mod':
    case 'modpack':
      return ['fabric', 'quilt', 'forge', 'neoforge'];
    case 'shader':
      return ['iris', 'optifine', 'canvas'];
    default:
      return [];
  }
}

/** The loader a search for this kind starts with, for an installation. */
export function loaderOf(kind: ModsKind, instance: InstanceSummary | null): string {
  if (instance?.loader == null || kind !== 'mod') return '';
  return instance.loader.kind;
}

/**
 * How a version goes into an installation: as its project's kind, except a
 * mod's data pack version (Terralith and the like publish both), which goes
 * into a world. The core decides the same.
 */
export function installKind(kind: ModsKind, version: ModsVersion): ModsKind {
  return kind === 'mod' &&
    version.loaders.length > 0 &&
    version.loaders.every((l) => l === 'datapack')
    ? 'datapack'
    : kind;
}

/** Kinds the launcher can put into an installation now. */
export function installable(kind: ModsKind): boolean {
  return (
    kind === 'mod' ||
    kind === 'resourcepack' ||
    kind === 'shader' ||
    kind === 'datapack' ||
    kind === 'world'
  );
}

/** The mods' loaders an installation's loader runs (Quilt runs Fabric's too). */
const MOD_LOADERS: Record<LoaderKind, readonly string[]> = {
  fabric: ['fabric'],
  quilt: ['quilt', 'fabric'],
  forge: ['forge'],
  neoforge: ['neoforge'],
  optifine: [],
};

/** At most this many game versions are named in a reason. */
const GAMES_NAMED = 4;

/**
 * Why a version cannot be added to the installation, in the player's words;
 * `null` when it can. The launcher's core refuses the same (`misfit` in
 * src-tauri/src/mods/content.rs); a game version that does not match only
 * warns for kinds other than mods.
 */
export function cannotAdd(
  kind: ModsKind,
  version: ModsVersion,
  instance: InstanceSummary | null,
): string | null {
  const t = strings.mods.project.cannotAdd;
  if (instance === null) return t.noInstallation;
  if (version.files.some((f) => f.held)) return t.held;
  if (version.files.length === 0) return t.noFile;
  if (kind !== 'mod') return null;
  const name = instance.displayName;
  const loader = instance.loader?.kind;
  if (loader === undefined) return t.noLoader({ name });
  if (loader === 'optifine') return t.optifineOnly({ name });
  if (version.loaders.length === 0) return t.noLoaders;
  const names = strings.mods.loaderNames as Record<string, string | undefined>;
  if (!version.loaders.some((l) => MOD_LOADERS[loader].includes(l))) {
    return t.wrongLoader({
      loaders: version.loaders.map((l) => names[l] ?? l).join(', '),
      loader: names[loader] ?? loader,
      name,
    });
  }
  if (!version.games.includes(instance.versionId)) {
    const games = version.games.slice(0, GAMES_NAMED).join(', ');
    return t.wrongGame({
      games: version.games.length > GAMES_NAMED ? `${games}…` : games,
      game: instance.versionId,
      name,
    });
  }
  return null;
}

/** A modpack's archive: Modrinth's `.mrpack` or CurseForge's `.zip`. */
export function packArchive(name: string): boolean {
  const lower = name.toLowerCase();
  return lower.endsWith('.mrpack') || lower.endsWith('.zip');
}

const FAILURES = [
  'bad_input',
  'curseforge_file_link',
  'nothing_found',
  'not_found',
  'timeout',
  'too_big',
  'unsupported_kind',
] as const;

/** Why a request failed, in the player's words. */
export function failureText(error: string | null): string {
  const t = strings.mods.requests.failures;
  return (FAILURES as readonly (string | null)[]).includes(error)
    ? t[error as (typeof FAILURES)[number]]
    : t.other;
}

export function stateTone(state: ModsImportRequest['state']): StatusTone {
  return state === 'ready'
    ? 'success'
    : state === 'failed'
      ? 'danger'
      : state === 'choosing'
        ? 'warning'
        : 'neutral';
}

/** A name or version kept whole inside a sentence of either direction. */
const isolate = (text: string) => `\u2068${text}\u2069`;

/** A problem the mods' jars report, in words (D-066). */
export function describeProblem(problem: ModsSetProblem): string {
  const t = strings.mods.problems;
  const names = t.names as Record<string, string | undefined>;
  const loaders = strings.mods.loaderNames as Record<string, string | undefined>;
  const mod = isolate(names[problem.id] ?? problem.id);
  const versions =
    problem.versions.length > 0 ? ` ${problem.versions.map(isolate).join(t.or)}` : '';
  const title = isolate(problem.titles[0] ?? '');
  const found = isolate(problem.found ?? '');
  switch (problem.kind) {
    case 'missing':
      return t.missing({ title, mod, versions });
    case 'wrong':
      return problem.from === null
        ? t.wrongHere({ title, mod, versions, found })
        : t.wrong({ title, mod, versions, found, from: isolate(problem.from) });
    case 'breaks':
      return problem.from === null
        ? t.breaksHere({ title, mod, found })
        : t.breaks({ title, from: isolate(problem.from) });
    case 'twice':
      return t.twice({ mod, files: problem.titles.map(isolate).join(', ') });
    case 'other_loader': {
      const loader = problem.loader ?? '';
      return t.otherLoader({ title, loader: loaders[loader] ?? loader });
    }
  }
}
