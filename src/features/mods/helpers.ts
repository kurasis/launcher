import type { StatusTone } from '../../components/StatusBadge';
import type { InstanceSummary, ModsImportRequest, ModsKind } from '../../lib/contracts';
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

/** Kinds the launcher can put into an installation now. */
export function installable(kind: ModsKind): boolean {
  return kind === 'mod' || kind === 'resourcepack' || kind === 'shader' || kind === 'datapack';
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
