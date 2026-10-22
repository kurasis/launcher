// The command palette's data (D-072, "Find · Ctrl K"): what it can open, how a typed
// query matches and ranks it, and which parts of a name to highlight. No React here.
import type { IconName } from '../components/Icon';
import type {
  AppSnapshot,
  ContentItem,
  InstanceSummary,
  LoaderKind,
  ModsHit,
  SettingsView,
} from '../lib/contracts';
import { type SettingsSection, strings } from '../lib/strings';
import { LOADER_KINDS, gameLabel } from '../features/instances/loaderLabel';
import { compact, ourPicture } from '../features/mods/helpers';
import type { ModsSection } from '../features/mods/ModsView';
import { type View, VIEW_ICONS, menuEntries, rowViews, viewShown } from './navigation';

/** The longest query, in characters: the catalog cuts its search text there too. */
export const MAX_QUERY = 200;

/** The catalog is asked only from this many characters on. */
export const CATALOG_MIN_QUERY = 2;

/** How many rows each group shows at most. */
export const CAPS = {
  installations: 6,
  installedMods: 5,
  catalogMods: 5,
  pages: 8,
} as const;

/** What choosing a row does; App carries it out. */
export type PaletteTarget =
  | { kind: 'view'; view: View }
  | { kind: 'console' }
  | { kind: 'settings'; section: SettingsSection }
  | { kind: 'modsSection'; section: ModsSection }
  | { kind: 'newInstallation'; loader: LoaderKind | null }
  | { kind: 'installation'; instanceId: string }
  | { kind: 'installedMod'; instanceId: string; projectId: string | null }
  | { kind: 'catalogMod'; projectId: string };

/** What a row shows before its name. */
export type PaletteLead =
  | { kind: 'icon'; icon: IconName }
  | { kind: 'tile' }
  | { kind: 'picture'; url: string | null; letter: string };

export interface PaletteEntry {
  /** Unique among all rows. */
  key: string;
  label: string;
  /** The muted second line: where it lives or what state it is in. */
  detail: string;
  /** What choosing it does, at the end of the row. */
  action: string;
  lead: PaletteLead;
  target: PaletteTarget;
}

// ---------------------------------------------------------------- the query

/** Bidirectional controls, zero-width joiners and other format characters. */
const FORMAT = /\p{Cf}/gu;
const CONTROL = /\p{Cc}/gu;
const SPACES = /\s+/gu;

/**
 * The query as it is searched: control characters out, runs of white space as
 * one space, trimmed, and at most `MAX_QUERY` characters.
 */
export function normalizeQuery(raw: string): string {
  const text = raw
    // A pasted wall of text is cut before any work on it.
    .slice(0, MAX_QUERY * 4)
    .replace(CONTROL, ' ')
    .replace(FORMAT, '')
    .replace(SPACES, ' ')
    .trim();
  return Array.from(text).slice(0, MAX_QUERY).join('').trimEnd();
}

// ---------------------------------------------------------------- matching

/** A highlighted span: [start, end) in UTF-16 units of the original text. */
export type Range = readonly [number, number];

const MARKS = /\p{M}/gu;
const LETTER_OR_DIGIT = /[\p{L}\p{N}]/u;
const TATWEEL = 'ـ';

/** One character as it is compared: lower case, without accents, marks and joiners. */
function foldPoint(point: string): string {
  if (/\s/u.test(point)) return ' ';
  if (point === TATWEEL || /[\p{Cc}\p{Cf}]/u.test(point)) return '';
  return point.toLowerCase().normalize('NFD').replace(MARKS, '');
}

interface Folded {
  text: string;
  /** For each UTF-16 unit of `text`: where its character starts in the original. */
  from: number[];
  /** ...and where it ends. */
  to: number[];
}

/**
 * The text folded character by character, so that a case change that alters the
 * length ('İ' is two units in lower case) never shifts the original positions.
 */
function fold(text: string): Folded {
  let folded = '';
  const from: number[] = [];
  const to: number[] = [];
  let index = 0;
  for (const point of text) {
    let piece = foldPoint(point);
    // Runs of spaces compare as one, as in the query.
    if (piece === ' ' && folded.endsWith(' ')) piece = '';
    for (let i = 0; i < piece.length; i += 1) {
      from.push(index);
      to.push(index + point.length);
    }
    folded += piece;
    index += point.length;
  }
  return { text: folded, from, to };
}

/** The query folded the same way: empty when nothing in it can match. */
function foldQuery(query: string): string {
  return fold(normalizeQuery(query)).text.trim();
}

/** Whether a query asks for nothing (empty, spaces or only marks). */
export function isEmptyQuery(query: string): boolean {
  return foldQuery(query) === '';
}

let graphemes: Intl.Segmenter | null | undefined;

/** Grapheme boundaries of `text`: a highlight never splits a letter from its marks. */
function boundaries(text: string): number[] | null {
  if (graphemes === undefined) {
    graphemes =
      typeof Intl.Segmenter === 'function'
        ? new Intl.Segmenter(undefined, { granularity: 'grapheme' })
        : null;
  }
  if (graphemes === null) return null;
  const found = Array.from(graphemes.segment(text), (segment) => segment.index);
  found.push(text.length);
  return found;
}

/** Widens [start, end) to whole graphemes, and over combining marks without a segmenter. */
function snap(text: string, start: number, end: number): Range {
  const bounds = boundaries(text);
  if (bounds === null) {
    let stop = end;
    while (stop < text.length && /\p{M}/u.test(text.charAt(stop))) stop += 1;
    return [start, stop];
  }
  let from = 0;
  let to = text.length;
  for (const bound of bounds) {
    if (bound <= start) from = bound;
    if (bound >= end) {
      to = bound;
      break;
    }
  }
  return [from, to];
}

function wordStartAt(folded: string, at: number): boolean {
  if (at === 0) return true;
  return !LETTER_OR_DIGIT.test(folded.charAt(at - 1));
}

/** How a query sits in a text: 0 at its start, 1 at a word's start, 2 inside a word. */
export interface TextMatch {
  rank: 0 | 1 | 2;
  range: Range;
}

/** Where `query` (already folded) best matches `text`, or null. */
function findFolded(text: string, query: string): TextMatch | null {
  if (query === '') return null;
  const folded = fold(text);
  const haystack = folded.text;
  let best: { at: number; rank: 0 | 1 | 2 } | null = null;
  if (haystack.startsWith(query)) {
    best = { at: 0, rank: 0 };
  } else {
    // Word starts first: one pass over the text, never a search per occurrence.
    for (let at = 1; at + query.length <= haystack.length; at += 1) {
      if (wordStartAt(haystack, at) && haystack.startsWith(query, at)) {
        best = { at, rank: 1 };
        break;
      }
    }
    if (best === null) {
      const at = haystack.indexOf(query);
      if (at !== -1) best = { at, rank: 2 };
    }
  }
  if (best === null) return null;
  const start = folded.from[best.at];
  const end = folded.to[best.at + query.length - 1];
  if (start === undefined || end === undefined) return null;
  return { rank: best.rank, range: snap(text, start, end) };
}

/** Where `query` best matches `text` ignoring case, accents and marks; null when it does not. */
export function matchText(text: string, query: string): TextMatch | null {
  return findFolded(text, foldQuery(query));
}

/**
 * How a query matches a row: the whole query in its name (ranks 0–2), in its second
 * line (3–5), or every word of it somewhere in either (6).
 */
export interface EntryMatch {
  rank: number;
  label: Range[];
  detail: Range[];
}

/** Words of a query matched one by one; more are ignored. */
const MAX_WORDS = 8;

function merge(ranges: Range[]): Range[] {
  const sorted = [...ranges].sort((a, b) => a[0] - b[0]);
  const merged: [number, number][] = [];
  for (const [start, end] of sorted) {
    const last = merged[merged.length - 1];
    if (last !== undefined && start <= last[1]) last[1] = Math.max(last[1], end);
    else merged.push([start, end]);
  }
  return merged;
}

/** How `query` matches a row with this name and second line; null when it does not. */
export function matchEntry(label: string, detail: string, query: string): EntryMatch | null {
  const folded = foldQuery(query);
  if (folded === '') return { rank: 0, label: [], detail: [] };
  const inLabel = findFolded(label, folded);
  if (inLabel !== null) return { rank: inLabel.rank, label: [inLabel.range], detail: [] };
  const inDetail = findFolded(detail, folded);
  if (inDetail !== null) return { rank: 3 + inDetail.rank, label: [], detail: [inDetail.range] };
  const words = folded.split(' ').filter((word) => word !== '');
  if (words.length < 2) return null;
  const labelRanges: Range[] = [];
  const detailRanges: Range[] = [];
  for (const word of words.slice(0, MAX_WORDS)) {
    const wordInLabel = findFolded(label, word);
    if (wordInLabel !== null) {
      labelRanges.push(wordInLabel.range);
      continue;
    }
    const wordInDetail = findFolded(detail, word);
    if (wordInDetail === null) return null;
    detailRanges.push(wordInDetail.range);
  }
  return { rank: 6, label: merge(labelRanges), detail: merge(detailRanges) };
}

/** A piece of a text, highlighted or not. */
export interface TextPart {
  text: string;
  mark: boolean;
}

/** `text` cut at the ranges, for drawing the highlighted parts as `<mark>`. */
export function highlightParts(text: string, ranges: readonly Range[]): TextPart[] {
  const parts: TextPart[] = [];
  let at = 0;
  for (const [rawStart, rawEnd] of merge([...ranges])) {
    const start = Math.max(at, Math.min(rawStart, text.length));
    const end = Math.max(start, Math.min(rawEnd, text.length));
    if (start > at) parts.push({ text: text.slice(at, start), mark: false });
    if (end > start) parts.push({ text: text.slice(start, end), mark: true });
    at = end;
  }
  if (at < text.length) parts.push({ text: text.slice(at), mark: false });
  return parts;
}

/** A row that matched, with what to highlight. */
export interface Ranked {
  entry: PaletteEntry;
  match: EntryMatch;
}

/**
 * The rows that match `query`, best first (start of the name, then a word's start,
 * then inside; the name before the second line), in their own order within a rank,
 * at most `cap`. An empty query keeps the first `cap` rows as they are.
 */
export function rankEntries(
  entries: readonly PaletteEntry[],
  query: string,
  cap: number,
): Ranked[] {
  const scored: (Ranked & { index: number })[] = [];
  entries.forEach((entry, index) => {
    const match = matchEntry(entry.label, entry.detail, query);
    if (match !== null) scored.push({ entry, match, index });
  });
  scored.sort((a, b) => a.match.rank - b.match.rank || a.index - b.index);
  return scored.slice(0, cap).map(({ entry, match }) => ({ entry, match }));
}

// ---------------------------------------------------------------- sources

/** Sections of the Settings screen, in its order. */
const SETTINGS_SECTIONS: Readonly<Record<SettingsSection, true>> = {
  general: true,
  feed: true,
  java: true,
  defaults: true,
  network: true,
  installations: true,
  privacy: true,
  reset: true,
};

/** Sections of the Mods screen, in its order. */
const MODS_SECTIONS: Readonly<Record<ModsSection, true>> = {
  mod: true,
  resourcepack: true,
  shader: true,
  datapack: true,
  modpack: true,
  world: true,
  installed: true,
  requests: true,
};

function keysOf<K extends string>(record: Readonly<Record<K, true>>): K[] {
  return Object.keys(record) as K[];
}

/**
 * Screens, the console, new installations (plain and with each loader) and the
 * sections of Settings and Mods. The feed only while it is on.
 */
export function pageEntries(settings: SettingsView | undefined): PaletteEntry[] {
  const t = strings.palette;
  const view = (item: View, detail: string): PaletteEntry => ({
    key: `view:${item}`,
    label: strings.nav[item],
    detail,
    action: t.go,
    lead: { kind: 'icon', icon: VIEW_ICONS[item] },
    target: { kind: 'view', view: item },
  });
  const entries: PaletteEntry[] = rowViews
    .filter((item) => viewShown(item, settings))
    .map((item) => view(item, t.screen));
  entries.push({
    key: 'new',
    label: strings.instances.create,
    detail: strings.nav.home,
    action: t.create,
    lead: { kind: 'icon', icon: 'plus' },
    target: { kind: 'newInstallation', loader: null },
  });
  for (const entry of menuEntries) {
    if (entry === 'separator') continue;
    if (entry === 'console') {
      entries.push({
        key: 'console',
        label: strings.console.open,
        detail: strings.nav.diagnostics,
        action: t.open,
        lead: { kind: 'icon', icon: VIEW_ICONS.console },
        target: { kind: 'console' },
      });
    } else {
      entries.push(view(entry, strings.nav.more));
    }
  }
  for (const loader of LOADER_KINDS) {
    entries.push({
      key: `new:${loader}`,
      label: t.newWith({ loader: strings.loaders.names[loader] }),
      detail: strings.nav.home,
      action: t.create,
      lead: { kind: 'icon', icon: 'plus' },
      target: { kind: 'newInstallation', loader },
    });
  }
  for (const section of keysOf(SETTINGS_SECTIONS)) {
    entries.push({
      key: `settings:${section}`,
      label: strings.settings.sections[section],
      detail: strings.nav.settings,
      action: t.go,
      lead: { kind: 'icon', icon: VIEW_ICONS.settings },
      target: { kind: 'settings', section },
    });
  }
  for (const section of keysOf(MODS_SECTIONS)) {
    entries.push({
      key: `mods:${section}`,
      label: strings.mods.sections[section],
      detail: strings.nav.mods,
      action: t.go,
      lead: { kind: 'icon', icon: VIEW_ICONS.mods },
      target: { kind: 'modsSection', section },
    });
  }
  return entries;
}

function playedAt(instance: InstanceSummary): number {
  if (instance.lastPlayedAt === null) return Number.NEGATIVE_INFINITY;
  const time = Date.parse(instance.lastPlayedAt);
  return Number.isNaN(time) ? Number.NEGATIVE_INFINITY : time;
}

/** Installations: the chosen one first, then the last played, then the rest as listed. */
export function installationOrder(snapshot: AppSnapshot): InstanceSummary[] {
  const chosen = snapshot.settings.selectedInstanceId;
  return snapshot.instances
    .map((instance, index) => ({ instance, index, played: playedAt(instance) }))
    .sort(
      (a, b) =>
        Number(b.instance.id === chosen) - Number(a.instance.id === chosen) ||
        (b.played === a.played ? 0 : b.played > a.played ? 1 : -1) ||
        a.index - b.index,
    )
    .map(({ instance }) => instance);
}

/** "Minecraft 1.21.1 · Fabric 0.16.9 · Ready", as in the launch bar. */
export function installationDetail(instance: InstanceSummary): string {
  return `${gameLabel(instance)} · ${strings.instances.health[instance.health]}`;
}

/** Installations as rows; `actionOf` names each one's main action. */
export function installationEntries(
  instances: readonly InstanceSummary[],
  actionOf: (instance: InstanceSummary) => string,
): PaletteEntry[] {
  return instances.map((instance): PaletteEntry => ({
    key: `installation:${instance.id}`,
    label: instance.displayName,
    detail: installationDetail(instance),
    action: actionOf(instance),
    lead: { kind: 'tile' },
    target: { kind: 'installation', instanceId: instance.id },
  }));
}

/** Whether an installation's mods are read: one with a loader that runs mods. */
export function holdsMods(instance: InstanceSummary): boolean {
  return instance.loader !== null && instance.loader.kind !== 'optifine';
}

/** The first letter or digit of a name, for a picture that is missing. */
function letterOf(name: string): string {
  return /[\p{L}\p{N}]/u.exec(name)?.[0] ?? '';
}

/** Mods in the mods folders of these installations (in their order). */
export function installedModEntries(
  instances: readonly InstanceSummary[],
  content: ReadonlyMap<string, readonly ContentItem[]>,
): PaletteEntry[] {
  const t = strings.palette;
  return instances.flatMap((instance) =>
    (content.get(instance.id) ?? [])
      .filter((item) => item.folder === 'mods')
      .map((item): PaletteEntry => {
        const label = item.title ?? item.name;
        const where = t.inInstallation({ name: instance.displayName });
        return {
          key: `mod:${instance.id}:${item.path}`,
          label,
          detail: item.dependency ? `${where} · ${strings.mods.content.dependency}` : where,
          action: t.open,
          lead: { kind: 'picture', url: ourPicture(item.icon), letter: letterOf(label) },
          target: { kind: 'installedMod', instanceId: instance.id, projectId: item.projectId },
        };
      }),
  );
}

/** Catalog hits as rows, without the projects already listed as installed. */
export function catalogEntries(
  hits: readonly ModsHit[],
  installed: ReadonlySet<string>,
): PaletteEntry[] {
  const t = strings.palette;
  const seen = new Set(installed);
  return hits
    .filter((hit) => {
      // Each project once, even when the catalog repeats one.
      if (seen.has(hit.id)) return false;
      seen.add(hit.id);
      return true;
    })
    .map((hit): PaletteEntry => {
      const label = hit.title || hit.slug;
      return {
        key: `catalog:${hit.id}`,
        label,
        detail: `${t.catalog} · ${strings.mods.downloads({ count: compact(hit.downloads) })}`,
        action: t.modPage,
        lead: { kind: 'picture', url: ourPicture(hit.icon), letter: letterOf(label) },
        target: { kind: 'catalogMod', projectId: hit.id },
      };
    });
}
