import { language } from '../../lib/strings';

/** Release notes in the interface language, its base language, or English. */
export function notesFor(notes: Record<string, string>): string | null {
  const tag = language();
  const base = tag.split('-')[0] ?? tag;
  return notes[tag] ?? notes[base] ?? notes.en ?? null;
}
