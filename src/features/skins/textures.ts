// What a skin or cape texture holds and how it is laid out (D-065). The core
// checks every file; these only draw and read what the player typed.
import type { SkinKind, Wardrobe } from '../../lib/contracts';

export const FRAME_MS_MIN = 50;
export const FRAME_MS_MAX = 10_000;
/** Duration of a frame the texture does not give. */
export const DEFAULT_FRAME_MS = 100;
export const MAX_FRAMES = 32;

/** A part of a flat view: where it is on a 64-wide texture, where it goes on the
 * figure (in the same pixels), and whether it is drawn mirrored. */
export interface Part {
  sx: number;
  sy: number;
  w: number;
  h: number;
  dx: number;
  dy: number;
  mirror?: boolean;
}

export type Side = 'front' | 'back';

/** One side of a skin on a 16×32 figure: head, body, arms and legs, then the outer
 * layer over them. The old 64×32 layout (and its HD sizes) has no left limbs (the
 * right ones, mirrored) and no outer layer but the hat. From behind, the player's
 * right arm and leg are on the right. */
export function skinParts(slim: boolean, legacy: boolean, side: Side = 'front'): Part[] {
  const arm = slim ? 3 : 4;
  if (side === 'back') {
    // The back face of a box starts two depths and a width after its corner.
    const base: Part[] = [
      { sx: 24, sy: 8, w: 8, h: 8, dx: 4, dy: 0 },
      { sx: 32, sy: 20, w: 8, h: 12, dx: 4, dy: 8 },
      { sx: 48 + arm, sy: 20, w: arm, h: 12, dx: 12, dy: 8 },
      legacy
        ? { sx: 48 + arm, sy: 20, w: arm, h: 12, dx: 4 - arm, dy: 8, mirror: true }
        : { sx: 40 + arm, sy: 52, w: arm, h: 12, dx: 4 - arm, dy: 8 },
      { sx: 12, sy: 20, w: 4, h: 12, dx: 8, dy: 20 },
      legacy
        ? { sx: 12, sy: 20, w: 4, h: 12, dx: 4, dy: 20, mirror: true }
        : { sx: 28, sy: 52, w: 4, h: 12, dx: 4, dy: 20 },
    ];
    const hat: Part = { sx: 56, sy: 8, w: 8, h: 8, dx: 4, dy: 0 };
    if (legacy) return [...base, hat];
    return [
      ...base,
      hat,
      { sx: 32, sy: 36, w: 8, h: 12, dx: 4, dy: 8 },
      { sx: 48 + arm, sy: 36, w: arm, h: 12, dx: 12, dy: 8 },
      { sx: 56 + arm, sy: 52, w: arm, h: 12, dx: 4 - arm, dy: 8 },
      { sx: 12, sy: 36, w: 4, h: 12, dx: 8, dy: 20 },
      { sx: 12, sy: 52, w: 4, h: 12, dx: 4, dy: 20 },
    ];
  }
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

/** A texture as the screen shows it: a vertical strip of frames. */
export interface Texture {
  /** The PNG, as a `data:` URL from the core. */
  src: string;
  kind: SkinKind;
  slim: boolean;
  width: number;
  height: number;
  frames: number;
  /** One per frame, one for all, or none (the default). */
  frameMs: readonly number[];
}

/** The old 64×32 skin layout (or its HD sizes): one frame, half as high as wide. */
export function isLegacy(texture: Pick<Texture, 'kind' | 'width' | 'height'>): boolean {
  return texture.kind === 'skin' && texture.height * 2 === texture.width;
}

/** Height of one frame in the strip. */
export function frameHeight(texture: Pick<Texture, 'kind' | 'width' | 'height'>): number {
  return texture.kind === 'skin' && !isLegacy(texture) ? texture.width : texture.width / 2;
}

/** How long each frame shows: one per frame, one for all, or the default. */
export function frameDurations(frames: number, frameMs: readonly number[]): number[] {
  const clamp = (ms: number) =>
    Number.isFinite(ms) ? Math.min(FRAME_MS_MAX, Math.max(FRAME_MS_MIN, ms)) : DEFAULT_FRAME_MS;
  const count = Number.isInteger(frames) ? Math.min(MAX_FRAMES, Math.max(1, frames)) : 1;
  if (frameMs.length === count) return frameMs.map(clamp);
  const one = frameMs.length === 1 ? clamp(frameMs[0] ?? DEFAULT_FRAME_MS) : DEFAULT_FRAME_MS;
  return Array.from({ length: count }, () => one);
}

/** The frame shown at `now`: frames follow the wall clock, as in the game with the
 * mod, so everyone sees the same frame at the same time. */
export function frameAt(durations: readonly number[], now: number): number {
  const total = durations.reduce((sum, ms) => sum + ms, 0);
  if (durations.length < 2 || total <= 0) return 0;
  let t = ((now % total) + total) % total;
  for (let i = 0; i < durations.length; i++) {
    const ms = durations[i] ?? 0;
    if (t < ms) return i;
    t -= ms;
  }
  return 0;
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

/** A PNG `data:` URL as the core sends them; anything else is not shown. */
export function isPngDataUrl(value: unknown): value is string {
  return typeof value === 'string' && value.startsWith('data:image/png;base64,');
}

/** Bytes of the file a base64 `data:` URL holds. */
export function dataUrlBytes(url: string): number {
  const comma = url.indexOf(',');
  if (comma < 0) return 0;
  const body = url.length - comma - 1;
  const padding = url.endsWith('==') ? 2 : url.endsWith('=') ? 1 : 0;
  return Math.max(0, Math.floor((body * 3) / 4) - padding);
}

/** The texture's frames, cut from its strip, at the texture's own size. */
export function cutFrames(image: CanvasImageSource, texture: Texture): HTMLCanvasElement[] {
  const height = frameHeight(texture);
  const count = Math.min(MAX_FRAMES, Math.max(1, texture.frames));
  return Array.from({ length: count }, (_, i) => {
    const canvas = document.createElement('canvas');
    canvas.width = texture.width;
    canvas.height = height;
    const context = canvas.getContext('2d');
    if (context !== null) {
      context.imageSmoothingEnabled = false;
      context.drawImage(image, 0, i * height, texture.width, height, 0, 0, texture.width, height);
    }
    return canvas;
  });
}

/** Loads a `data:` URL as an image; rejects when it does not decode. */
export function loadImage(src: string): Promise<HTMLImageElement> {
  return new Promise((resolve, reject) => {
    const image = new Image();
    image.onload = () => {
      resolve(image);
    };
    image.onerror = () => {
      reject(new Error('The texture does not decode.'));
    };
    image.src = src;
  });
}

/** Whether a skin frame has slim arms: the strips a classic arm fills and a slim one
 * leaves empty are transparent (the same test skin sites use). */
export function looksSlim(frame: HTMLCanvasElement): boolean {
  const context = frame.getContext('2d');
  if (context === null || frame.width < 64 || frame.height !== frame.width) return false;
  const k = frame.width / 64;
  const empty = (x: number, y: number, w: number, h: number) => {
    const data = context.getImageData(x * k, y * k, w * k, h * k).data;
    for (let i = 3; i < data.length; i += 4) if (data[i] === 0) return true;
    return false;
  };
  return empty(50, 16, 2, 4) || empty(54, 20, 2, 12) || empty(42, 48, 2, 4) || empty(46, 52, 2, 12);
}

type Rgb = readonly [number, number, number];

/** A plain figure for "how players without the mod see you": the game shows its own
 * skin there, which the launcher may not copy, so this stands in for it. */
export function plainSkin(): HTMLCanvasElement {
  const canvas = document.createElement('canvas');
  canvas.width = 64;
  canvas.height = 64;
  const context = canvas.getContext('2d');
  if (context === null) return canvas;
  const fill = (color: Rgb, x: number, y: number, w: number, h: number) => {
    context.fillStyle = `rgb(${String(color[0])} ${String(color[1])} ${String(color[2])})`;
    context.fillRect(x, y, w, h);
  };
  const skin: Rgb = [196, 160, 132];
  const hair: Rgb = [92, 84, 80];
  const shirt: Rgb = [120, 134, 156];
  const pants: Rgb = [78, 86, 104];
  const shoes: Rgb = [58, 60, 66];
  // Head: skin all round, hair on top and at the back.
  fill(skin, 0, 0, 32, 16);
  fill(hair, 8, 0, 8, 8);
  fill(hair, 24, 8, 8, 8);
  fill(hair, 0, 8, 24, 2);
  fill([250, 250, 250], 9, 12, 2, 1);
  fill([250, 250, 250], 13, 12, 2, 1);
  fill([60, 64, 80], 10, 12, 1, 1);
  fill([60, 64, 80], 13, 12, 1, 1);
  // Body, both arms (sleeves to the elbow) and both legs.
  fill(shirt, 16, 16, 24, 16);
  for (const [x, y] of [
    [40, 16],
    [32, 48],
  ] as const) {
    fill(skin, x, y, 16, 16);
    fill(shirt, x + 4, y, 4, 4);
    fill(shirt, x, y + 4, 16, 4);
  }
  for (const [x, y] of [
    [0, 16],
    [16, 48],
  ] as const) {
    fill(pants, x, y, 16, 16);
    fill(shoes, x, y + 13, 16, 3);
  }
  return canvas;
}

/** The OS setting "reduce motion": frames and the character stay still. */
export function reducedMotion(): boolean {
  return typeof window.matchMedia === 'function'
    ? window.matchMedia('(prefers-reduced-motion: reduce)').matches
    : false;
}
