// The wardrobe (D-065): what the account keeps, how the screen orders and names it,
// and what trying something on changes. The core checks every answer of the service;
// these keep the screen safe from anything else.
import {
  type Closet,
  type SkinKind,
  type SkinUploads,
  type WardrobeChange,
  type WardrobeItem,
  type WardrobeSet,
  type WornTexture,
  WARDROBE_NAME_CHARS,
} from '../../lib/contracts';
import { numberLocale, strings } from '../../lib/strings';
import { type Texture, frameHeight } from './textures';

/** The wardrobe's tabs: skins, capes and sets. */
export type WardrobeTab = SkinKind | 'set';
export const WARDROBE_TABS: readonly WardrobeTab[] = ['skin', 'cape', 'set'];

export type WardrobeSort = 'worn' | 'new' | 'name' | 'favourite';
export const WARDROBE_SORTS: readonly WardrobeSort[] = ['worn', 'new', 'name', 'favourite'];

/** What is being tried on: kept items (a skin, a cape or both), or a set. */
export type Trial =
  { of: 'items'; skin: string | null; cape: string | null } | { of: 'set'; id: string };

/** Uploads an account may make in an hour (the service's rule). */
export const UPLOADS_PER_HOUR = 10;

/** The most of each the core passes on; more is not a wardrobe. */
const MAX_KEPT = 128;
/** The largest limit the core passes on. */
const MAX_LIMIT = 1000;

/** Either a skin or a cape kept, or a set. */
type Entry = WardrobeItem | WardrobeSet;

const isRecord = (value: unknown): value is Record<string, unknown> =>
  typeof value === 'object' && value !== null && !Array.isArray(value);

const isId = (value: unknown): value is string =>
  typeof value === 'string' && /^[0-9a-f]{32}$/.test(value);

/** Characters as the service counts them: code points (Rust's `chars`). */
export const charCount = (text: string): number => Array.from(text).length;

/** The first `count` characters, cut between code points (never inside one). */
export const firstChars = (text: string, count: number): string =>
  Array.from(text).slice(0, count).join('');

const isTime = (value: unknown): value is number =>
  typeof value === 'number' && Number.isSafeInteger(value) && value >= 0;

/** A texture as the core describes one: a SHA-256 and a strip of 1–32 frames. */
export function isWornTexture(value: unknown): value is WornTexture {
  if (!isRecord(value)) return false;
  const { hash, slim, width, height, frames, frameMs } = value;
  return (
    typeof hash === 'string' &&
    /^[0-9a-f]{64}$/.test(hash) &&
    typeof slim === 'boolean' &&
    [64, 128, 256, 512].includes(width as number) &&
    typeof height === 'number' &&
    Number.isInteger(height) &&
    height > 0 &&
    typeof frames === 'number' &&
    Number.isInteger(frames) &&
    frames >= 1 &&
    frames <= 32 &&
    Array.isArray(frameMs) &&
    frameMs.length <= 32 &&
    frameMs.every((ms) => typeof ms === 'number' && Number.isFinite(ms))
  );
}

function isPiece(value: unknown): value is WornTexture | null {
  return value === null || isWornTexture(value);
}

function isItem(value: unknown): value is WardrobeItem {
  if (!isRecord(value)) return false;
  return (
    isId(value.id) &&
    (value.kind === 'skin' || value.kind === 'cape') &&
    typeof value.name === 'string' &&
    charCount(value.name) <= WARDROBE_NAME_CHARS &&
    typeof value.favourite === 'boolean' &&
    isTime(value.created) &&
    isTime(value.worn) &&
    isWornTexture(value.texture)
  );
}

function isSet(value: unknown): value is WardrobeSet {
  if (!isRecord(value)) return false;
  return (
    isId(value.id) &&
    typeof value.name === 'string' &&
    charCount(value.name) <= WARDROBE_NAME_CHARS &&
    typeof value.favourite === 'boolean' &&
    isTime(value.created) &&
    isTime(value.worn) &&
    isPiece(value.skin) &&
    isPiece(value.cape) &&
    (value.skin !== null || value.cape !== null)
  );
}

const isLimit = (value: unknown): value is number =>
  typeof value === 'number' && Number.isInteger(value) && value >= 0 && value <= MAX_LIMIT;

/** The wardrobe the core answered, or null for anything else. Entries that do not
 * read as the core's are left out rather than shown wrong. */
export function closetOf(value: unknown): Closet | null {
  if (!isRecord(value)) return null;
  const { worn, items, sets, limits } = value;
  if (
    !isRecord(worn) ||
    !isPiece(worn.skin ?? null) ||
    !isPiece(worn.cape ?? null) ||
    !Array.isArray(items) ||
    !Array.isArray(sets) ||
    !isRecord(limits) ||
    !isLimit(limits.skins) ||
    !isLimit(limits.capes) ||
    !isLimit(limits.sets)
  ) {
    return null;
  }
  const uploads = isRecord(worn.uploads) ? worn.uploads : null;
  return {
    worn: {
      skin: (worn.skin ?? null) as WornTexture | null,
      cape: (worn.cape ?? null) as WornTexture | null,
      uploads:
        uploads !== null && isLimit(uploads.left) && isTime(uploads.resetSeconds)
          ? { left: uploads.left, resetSeconds: Math.min(uploads.resetSeconds, 24 * 3600) }
          : null,
    },
    items: items.slice(0, MAX_KEPT).filter(isItem),
    sets: sets.slice(0, MAX_KEPT).filter(isSet),
    limits: { skins: limits.skins, capes: limits.capes, sets: limits.sets },
  };
}

/** A texture's facts on its card: HD, frames and arms, or just its size. */
export function chipsOf(kind: SkinKind, texture: WornTexture): string[] {
  const f = strings.accounts.skins.facts;
  const chips: string[] = [];
  if (texture.width > 64) chips.push(f.hd({ scale: texture.width / 64 }));
  if (texture.frames > 1) chips.push(f.frames({ count: texture.frames }));
  if (kind === 'skin' && texture.slim) chips.push(strings.accounts.skins.wardrobe.slimArms);
  if (chips.length === 0) {
    chips.push(f.size({ width: texture.width, height: frameHeight({ kind, ...texture }) }));
  }
  return chips;
}

/** A name to show: the one given, or the kind and the day it was kept. */
export function displayName(entry: Entry): string {
  const name = entry.name.trim();
  if (name !== '') return name;
  const date = new Date(entry.created * 1000).toLocaleDateString(numberLocale(), {
    day: 'numeric',
    month: 'short',
  });
  const u = strings.accounts.skins.wardrobe.unnamed;
  return 'kind' in entry ? u[entry.kind]({ date }) : u.set({ date });
}

/** The items of a kind, or the sets, in the order chosen. */
export function sortedEntries<T extends Entry>(list: readonly T[], sort: WardrobeSort): T[] {
  const newest = (a: T, b: T) => b.created - a.created || a.id.localeCompare(b.id);
  const recent = (a: T, b: T) => b.worn - a.worn || newest(a, b);
  const order: Record<WardrobeSort, (a: T, b: T) => number> = {
    worn: recent,
    new: newest,
    name: (a, b) =>
      displayName(a).localeCompare(displayName(b), numberLocale(), {
        sensitivity: 'base',
        numeric: true,
      }) || newest(a, b),
    favourite: (a, b) => Number(b.favourite) - Number(a.favourite) || recent(a, b),
  };
  return [...list].sort(order[sort]);
}

/** The same texture, worn the same way. */
export function sameTexture(a: WornTexture | null, b: WornTexture | null): boolean {
  if (a === null || b === null) return a === b;
  return a.hash === b.hash && a.slim === b.slim;
}

/** Each kind as the trial would leave it: a texture, nothing (taken off), or
 * undefined where what is worn stays. */
export interface TrialPieces {
  skin: WornTexture | null | undefined;
  cape: WornTexture | null | undefined;
  /** The names to say what is tried on. */
  names: { skin: string | null; cape: string | null; set: string | null };
}

/** What a trial changes against what is worn; null when it changes nothing (an item
 * tried on that is the one worn) or names nothing the wardrobe still has. */
export function trialPieces(closet: Closet, trial: Trial | null): TrialPieces | null {
  if (trial === null) return null;
  if (trial.of === 'set') {
    const set = closet.sets.find((s) => s.id === trial.id);
    if (set === undefined) return null;
    return {
      skin: set.skin,
      cape: set.cape,
      names: { skin: null, cape: null, set: displayName(set) },
    };
  }
  const piece = (kind: SkinKind) => {
    const id = trial[kind];
    const item = closet.items.find((i) => i.id === id && i.kind === kind);
    if (item === undefined || sameTexture(item.texture, closet.worn[kind])) return null;
    return item;
  };
  const skin = piece('skin');
  const cape = piece('cape');
  if (skin === null && cape === null) return null;
  return {
    skin: skin?.texture,
    cape: cape?.texture,
    names: {
      skin: skin === null ? null : displayName(skin),
      cape: cape === null ? null : displayName(cape),
      set: null,
    },
  };
}

/** The changes that put the trial on: wearing from the wardrobe is never an upload. */
export function trialChanges(closet: Closet, trial: Trial | null): WardrobeChange[] {
  const pieces = trialPieces(closet, trial);
  if (pieces === null || trial === null) return [];
  if (trial.of === 'set') return [{ action: 'wear', of: 'set', id: trial.id }];
  const changes: WardrobeChange[] = [];
  for (const kind of ['skin', 'cape'] as const) {
    const id = trial[kind];
    if (id !== null && pieces[kind] !== undefined) changes.push({ action: 'wear', of: 'item', id });
  }
  return changes;
}

/** Trying an item on, or taking it off the trial when it is tried on already. */
export function toggleItem(trial: Trial | null, item: WardrobeItem): Trial | null {
  const base = trial?.of === 'items' ? trial : { of: 'items' as const, skin: null, cape: null };
  const next = { ...base, [item.kind]: base[item.kind] === item.id ? null : item.id };
  return next.skin === null && next.cape === null ? null : next;
}

/** Trying a set on, or not. */
export function toggleSet(trial: Trial | null, set: WardrobeSet): Trial | null {
  return trial?.of === 'set' && trial.id === set.id ? null : { of: 'set', id: set.id };
}

/** Whether an item or set is tried on now. */
export function isTried(trial: Trial | null, entry: Entry): boolean {
  if (trial === null) return false;
  if ('kind' in entry) return trial.of === 'items' && trial[entry.kind] === entry.id;
  return trial.of === 'set' && trial.id === entry.id;
}

/** How many a wardrobe keeps of a kind, and how many it may. */
export function room(closet: Closet, tab: WardrobeTab): { count: number; limit: number } {
  if (tab === 'set') return { count: closet.sets.length, limit: closet.limits.sets };
  return {
    count: closet.items.filter((item) => item.kind === tab).length,
    limit: tab === 'skin' ? closet.limits.skins : closet.limits.capes,
  };
}

/** A texture the screen can draw, once its PNG is fetched ('' when it could not be). */
export function textureOf(
  kind: SkinKind,
  worn: WornTexture | null | undefined,
  src: string | undefined,
): Texture | null {
  if (worn === null || worn === undefined || src === undefined || src === '') return null;
  return {
    src,
    kind,
    slim: kind === 'skin' && worn.slim,
    width: worn.width,
    height: worn.height,
    frames: worn.frames,
    frameMs: worn.frameMs,
  };
}

/** Characters a wardrobe name never holds: control, invisible and direction ones
 * (the service's rule). */
// eslint-disable-next-line no-control-regex
const HIDDEN = /[\u0000-\u001f\u007f-\u009f\u200b-\u200f\u202a-\u202e\u2066-\u2069\ufeff]/;

/** A name as typed, as the service keeps it: trimmed, up to 32 characters, without
 * control, invisible or direction characters; null when it cannot be one. */
export function wardrobeName(text: string): string | null {
  const name = text.trim();
  if (HIDDEN.test(name)) return null;
  return charCount(name) <= WARDROBE_NAME_CHARS ? name : null;
}

/** A name for something picked: the file's name without its ending, cleaned and cut
 * to the rule (the player can change it). */
export function nameFromFile(file: string): string {
  const plain = file.replace(/\.(png|gif|apng)$/i, '').replace(new RegExp(HIDDEN, 'g'), '');
  return firstChars(plain.trim(), WARDROBE_NAME_CHARS).trim();
}

/** What is left of the hour's uploads, and when the count starts over once none is. */
export function uploadsLine(uploads: SkinUploads | null): string {
  const w = strings.accounts.skins.wardrobe;
  if (uploads === null) return w.uploadsUnknown;
  const left = w.uploadsLeft({ left: uploads.left, total: UPLOADS_PER_HOUR });
  if (uploads.left > 0 || uploads.resetSeconds <= 0) return left;
  return `${left} ${w.uploadsReset({ minutes: Math.max(1, Math.ceil(uploads.resetSeconds / 60)) })}`;
}
