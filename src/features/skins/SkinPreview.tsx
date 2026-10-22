import { useEffect, useRef } from 'react';
import styles from './Skins.module.css';
import {
  CAPE_PARTS,
  type Side,
  type Texture,
  frameDurations,
  frameHeight,
  isLegacy,
  reducedMotion,
  skinParts,
} from './textures';

/** Canvas pixels per texture pixel of a standard (64-wide) texture. */
const UNIT = 8;

interface Props extends Texture {
  label: string;
  /** Which side of a skin; a cape is always seen from behind. */
  side?: Side;
  /** One frame only, held still; otherwise the frames show in turn. */
  frame?: number | undefined;
  /** A smaller drawing, for the strip of frames. */
  small?: boolean;
}

/** A flat view of a skin or cape, its frames shown in turn. Also what the screen
 * shows when the 3D view cannot run. */
export function SkinPreview({ label, side = 'front', frame, small = false, ...texture }: Props) {
  const canvas = useRef<HTMLCanvasElement>(null);
  const { src, kind, slim, width, frames, frameMs } = texture;
  const scale = width / 64;
  const legacy = isLegacy(texture);
  const height = frameHeight(texture);
  const figure = kind === 'skin' ? { w: 16, h: 32 } : { w: 10, h: 16 };
  const durations = frameDurations(frames, frameMs).join(',');

  useEffect(() => {
    const target = canvas.current;
    if (target === null) return;
    const parts = kind === 'skin' ? skinParts(slim, legacy, side) : CAPE_PARTS;
    const times = durations.split(',').map(Number);
    let timer: ReturnType<typeof setTimeout> | undefined;
    let live = true;
    const image = new Image();

    const draw = (shown: number) => {
      const context = target.getContext('2d');
      if (context === null) return;
      context.clearRect(0, 0, target.width, target.height);
      context.imageSmoothingEnabled = false;
      const top = shown * height;
      for (const p of parts) {
        const sx = p.sx * scale;
        const sy = top + p.sy * scale;
        const sw = p.w * scale;
        const sh = p.h * scale;
        if (p.mirror === true) {
          context.save();
          context.translate((p.dx + p.w) * UNIT, p.dy * UNIT);
          context.scale(-1, 1);
          context.drawImage(image, sx, sy, sw, sh, 0, 0, p.w * UNIT, p.h * UNIT);
          context.restore();
        } else {
          context.drawImage(
            image,
            sx,
            sy,
            sw,
            sh,
            p.dx * UNIT,
            p.dy * UNIT,
            p.w * UNIT,
            p.h * UNIT,
          );
        }
      }
    };

    const show = (shown: number) => {
      if (!live) return;
      draw(shown);
      if (frame === undefined && times.length > 1 && !reducedMotion()) {
        timer = setTimeout(() => {
          show((shown + 1) % times.length);
        }, times[shown]);
      }
    };

    image.onload = () => {
      show(frame === undefined ? 0 : Math.min(Math.max(0, frame), times.length - 1));
    };
    image.src = src;
    return () => {
      live = false;
      image.onload = null;
      if (timer !== undefined) clearTimeout(timer);
    };
  }, [src, kind, slim, legacy, side, scale, height, durations, frame]);

  return (
    <canvas
      ref={canvas}
      className={[kind === 'skin' ? styles.skinPreview : styles.capePreview, small && styles.small]
        .filter(Boolean)
        .join(' ')}
      width={figure.w * UNIT}
      height={figure.h * UNIT}
      role="img"
      aria-label={label}
    />
  );
}
