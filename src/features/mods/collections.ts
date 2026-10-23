import type { ModsCollection } from '../../lib/contracts';
import { language, strings } from '../../lib/strings';

const PRESETS = ['popular-week', 'new', 'players-packs'] as const;
type Preset = (typeof PRESETS)[number];
const isPreset = (id: string): id is Preset => (PRESETS as readonly string[]).includes(id);

/** A text of the owner's in the interface language, else in English, else any. */
function textOf(texts: Record<string, string> | null | undefined): string | null {
  if (texts === null || typeof texts !== 'object') return null;
  const lang = language().split('-')[0] ?? 'en';
  const found = [texts[lang], texts.en, ...Object.values(texts)].find(
    (text): text is string => typeof text === 'string' && text.trim() !== '',
  );
  return found ?? null;
}

/** A collection's name as the player reads it. */
export function collectionTitle(collection: ModsCollection): string {
  const t = strings.mods.catalog;
  if (collection.preset && isPreset(collection.id)) return t.presets[collection.id];
  return textOf(collection.titles) ?? collection.id;
}

/** A line on what a collection holds. */
export function collectionNote(collection: ModsCollection): string {
  const t = strings.mods.catalog;
  const owners = collection.preset ? null : textOf(collection.notes);
  if (owners !== null) return owners;
  if (collection.preset && isPreset(collection.id)) {
    const notes = t.presetNotes;
    if (collection.id === 'new') return notes.new({ days: collection.query.since ?? 30 });
    return notes[collection.id];
  }
  const names = collection.names.filter((name) => typeof name === 'string' && name !== '');
  if (names.length === 0) return strings.mods.results({ count: collection.count });
  const more = collection.count - names.length;
  return more > 0 ? t.andMore({ names: names.join(', '), count: more }) : names.join(', ');
}
