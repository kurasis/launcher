// What a skin or cape texture holds and how its front is laid out (D-065). The
// core checks every file; these only draw and read what the player typed.
import type { Wardrobe } from '../../lib/contracts';

export const FRAME_MS_MIN = 50;
export const FRAME_MS_MAX = 10_000;
/** Duration of a frame the texture does not give. */
export const DEFAULT_FRAME_MS = 100;

/** A part of the front view: where it is on a 64-wide texture, where it goes on the
 * figure (in the same pixels), and whether it is the other side's part mirrored. */
export interface Part {
  sx: number;
  sy: number;
  w: number;
  h: number;
  dx: number;
  dy: number;
  mirror?: boolean;
}

/** The front of a skin on a 16×32 figure: head, body, arms and legs, then the outer
 * layer over them. The old 64×32 layout (and its HD sizes) has no left limbs (the right ones, mirrored)
 * and no outer layer but the hat. */
export function skinParts(slim: boolean, legacy: boolean): Part[] {
  const arm = slim ? 3 : 4;
  const base: Part[] = [
    { sx: 8, sy: 8, w: 8, h: 8, dx: 4, dy: 0 },
    { sx: 20, sy: 20, w: 8, h: 12, dx: 4, dy: 8 },
    { sx: 44, sy: 20, w: arm, h: 12, dx: 4 - arm, dy: 8 },
    legacy
      ? { sx: 44, sy: 20, w: arm, h: 12, dx: 12, dy: 8, mirror: true }
      : { sx: 36, sy: 52, w: arm, h: 12, dx: 12, dy: 8 },
    { sx: 4, sy: 20, w: 4, h: 12, dx: 4, dy: 20 },
    legacy
      ? { sx: 4, sy: 20, w: 4, h: 12, dx: 8, dy: 20, mirror: true }
      : { sx: 20, sy: 52, w: 4, h: 12, dx: 8, dy: 20 },
  ];
  const hat: Part = { sx: 40, sy: 8, w: 8, h: 8, dx: 4, dy: 0 };
  if (legacy) return [...base, hat];
  return [
    ...base,
    hat,
    { sx: 20, sy: 36, w: 8, h: 12, dx: 4, dy: 8 },
    { sx: 44, sy: 36, w: arm, h: 12, dx: 4 - arm, dy: 8 },
    { sx: 52, sy: 52, w: arm, h: 12, dx: 12, dy: 8 },
    { sx: 4, sy: 36, w: 4, h: 12, dx: 4, dy: 20 },
    { sx: 4, sy: 52, w: 4, h: 12, dx: 8, dy: 20 },
  ];
}

/** The outside of a cape on a 10×16 figure, as seen from behind the player. */
export const CAPE_PARTS: readonly Part[] = [{ sx: 1, sy: 1, w: 10, h: 16, dx: 0, dy: 0 }];

/** How long each frame shows: one per frame, one for all, or the default. */
export function frameDurations(frames: number, frameMs: readonly number[]): number[] {
  const clamp = (ms: number) =>
    Number.isFinite(ms) ? Math.min(FRAME_MS_MAX, Math.max(FRAME_MS_MIN, ms)) : DEFAULT_FRAME_MS;
  const count = Number.isInteger(frames) ? Math.min(32, Math.max(1, frames)) : 1;
  if (frameMs.length === count) return frameMs.map(clamp);
  const one = frameMs.length === 1 ? clamp(frameMs[0] ?? DEFAULT_FRAME_MS) : DEFAULT_FRAME_MS;
  return Array.from({ length: count }, () => one);
}

/** A frame duration as typed: whole milliseconds within the rule, else null. */
export function parseFrameMs(text: string): number | null {
  const trimmed = text.trim();
  if (!/^\d{1,5}$/.test(trimmed)) return null;
  const ms = Number(trimmed);
  return ms >= FRAME_MS_MIN && ms <= FRAME_MS_MAX ? ms : null;
}

/** What the core answered is a wardrobe (both slots present). */
export function isWardrobe(value: unknown): value is Wardrobe {
  return typeof value === 'object' && value !== null && 'skin' in value && 'cape' in value;
}
