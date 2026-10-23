import type { ImportNote, PickedTexture } from '../../lib/contracts';
import { DEFAULT_FRAME_MS, MAX_FRAMES, type Texture, isPngDataUrl, parseFrameMs } from './textures';
import { nameFromFile } from './wardrobe';

/** A picked file being looked at before it is worn. */
export interface Draft {
  picked: PickedTexture;
  /** Its name in the wardrobe, as typed. */
  name: string;
  /** Skins only: the slim (3-pixel) arms. */
  slim: boolean;
  /** What the file looks like it has, when the frames could be read. */
  slimGuess: boolean | null;
  /** One duration for every frame. */
  same: boolean;
  sameText: string;
  /** One per frame, as typed. */
  texts: string[];
  /** A frame held still, or null while the frames play. */
  frame: number | null;
}

export function newDraft(picked: PickedTexture, slim: boolean): Draft {
  const fromFile = picked.frameMs.length === picked.frames ? picked.frameMs : [];
  const texts = Array.from({ length: picked.frames }, (_, i) =>
    String(fromFile[i] ?? DEFAULT_FRAME_MS),
  );
  const same = texts.every((t) => t === texts[0]);
  return {
    picked,
    name: nameFromFile(picked.name),
    slim,
    slimGuess: null,
    same,
    sameText: texts[0] ?? String(DEFAULT_FRAME_MS),
    texts,
    frame: null,
  };
}

/** The durations as typed, each within the rule or null. */
export function typedDurations(draft: Draft): (number | null)[] {
  return draft.same
    ? draft.texts.map(() => parseFrameMs(draft.sameText))
    : draft.texts.map(parseFrameMs);
}

/** What goes to the service: nothing for one frame, one duration for all, or one
 * per frame; null while a duration is not valid. */
export function durationsToSend(draft: Draft): number[] | null | undefined {
  if (draft.picked.frames <= 1) return null;
  const typed = typedDurations(draft);
  if (typed.some((ms) => ms === null)) return undefined;
  const list = typed as number[];
  return list.every((ms) => ms === list[0]) ? [list[0] ?? DEFAULT_FRAME_MS] : list;
}

/** The picked file as the character wears it while it is tried on. */
export function trialTexture(draft: Draft): Texture {
  const { picked } = draft;
  return {
    src: picked.preview,
    kind: picked.kind,
    slim: picked.kind === 'skin' && draft.slim,
    width: picked.width,
    height: picked.height,
    frames: picked.frames,
    frameMs: typedDurations(draft).map((ms) => ms ?? DEFAULT_FRAME_MS),
  };
}

const isCount = (value: unknown, max: number): value is number =>
  typeof value === 'number' && Number.isInteger(value) && value >= 0 && value <= max;

function isNote(value: unknown): value is ImportNote {
  if (typeof value !== 'object' || value === null) return false;
  const note = value as Record<string, unknown>;
  switch (note.kind) {
    case 'merged':
      return isCount(note.from, 10_000) && isCount(note.to, 10_000);
    case 'everyNth':
      return isCount(note.from, 10_000) && isCount(note.to, 10_000) && isCount(note.step, 10_000);
    case 'clamped':
      return (
        isCount(note.frame, 10_000) && isCount(note.was, 1_000_000) && isCount(note.now, 1_000_000)
      );
    default:
      return false;
  }
}

/** A file the core checked, as it describes one; null for anything else. Its picture
 * must be a PNG `data:` URL, so nothing is ever loaded from elsewhere. */
export function pickedOf(value: unknown): PickedTexture | null {
  if (typeof value !== 'object' || value === null) return null;
  const p = value as Record<string, unknown>;
  const ok =
    (p.kind === 'skin' || p.kind === 'cape') &&
    typeof p.name === 'string' &&
    p.name.length <= 1024 &&
    [64, 128, 256, 512].includes(p.width as number) &&
    isCount(p.height, 512 * MAX_FRAMES) &&
    p.height > 0 &&
    isCount(p.frames, MAX_FRAMES) &&
    p.frames >= 1 &&
    Array.isArray(p.frameMs) &&
    p.frameMs.length <= MAX_FRAMES &&
    p.frameMs.every((ms) => isCount(ms, 1_000_000)) &&
    (p.source === 'png' || p.source === 'apng' || p.source === 'gif') &&
    Array.isArray(p.notes) &&
    p.notes.length <= 64 &&
    p.notes.every(isNote) &&
    isCount(p.bytes, 8 * 1024 * 1024) &&
    isPngDataUrl(p.preview) &&
    (p.slim === null || typeof p.slim === 'boolean');
  return ok ? (value as PickedTexture) : null;
}
