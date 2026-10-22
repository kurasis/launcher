import type { PickedTexture } from '../../lib/contracts';
import { DEFAULT_FRAME_MS, type Texture, parseFrameMs } from './textures';

/** A picked file being looked at before it is worn. */
export interface Draft {
  picked: PickedTexture;
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
