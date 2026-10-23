import { language } from '../../lib/strings';

/** Release notes in the interface language, its base language, or English. */
export function notesFor(notes: Record<string, string>): string | null {
  const tag = language();
  const base = tag.split('-')[0] ?? tag;
  return notes[tag] ?? notes[base] ?? notes.en ?? null;
}

/** The most lines of notes shown as a list; a longer text is cut there. */
const MAX_LINES = 40;

/**
 * Release notes as lines, one item each: line breaks split them and a leading "-", "*"
 * or "•" goes. The text stays text: nothing in it becomes markup or a link.
 */
export function noteLines(notes: string): string[] {
  return notes
    .split(/\r?\n/)
    .map((line) => line.trim().replace(/^[-*•]\s+/, ''))
    .filter((line) => line !== '')
    .slice(0, MAX_LINES);
}
