import { useEffect, useRef } from 'react';
import type { SkinKind } from '../../lib/contracts';
import styles from './Accounts.module.css';
import { CAPE_PARTS, frameDurations, skinParts } from './skinTextures';

/** Canvas pixels per texture pixel of a standard (64-wide) texture. */
const UNIT = 8;

function reducedMotion(): boolean {
  return typeof window.matchMedia === 'function'
    ? window.matchMedia('(prefers-reduced-motion: reduce)').matches
    : false;
}

interface Props {
  /** The PNG, as a `data:` URL. */
  src: string;
  kind: SkinKind;
  slim: boolean;
  width: number;
  height: number;
  frames: number;
  frameMs: readonly number[];
  label: string;
}

/** A flat front view of a skin or cape, its frames shown in turn. */
export function SkinPreview({ src, kind, slim, width, height, frames, frameMs, label }: Props) {
  const canvas = useRef<HTMLCanvasElement>(null);
  const scale = width / 64;
  // The layout before 1.8: 64×32, or HD like 512×256, always one frame.
  const legacy = kind === 'skin' && height * 2 === width;
  const frameHeight = kind === 'skin' && !legacy ? width : width / 2;
  const figure = kind === 'skin' ? { w: 16, h: 32 } : { w: 10, h: 16 };
  const durations = frameDurations(frames, frameMs).join(',');

  useEffect(() => {
    const target = canvas.current;
    if (target === null) return;
    const parts = kind === 'skin' ? skinParts(slim, legacy) : CAPE_PARTS;
    const times = durations.split(',').map(Number);
    let timer: ReturnType<typeof setTimeout> | undefined;
    let live = true;
    const image = new Image();

    const draw = (frame: number) => {
      const context = target.getContext('2d');
      if (context === null) return;
      context.clearRect(0, 0, target.width, target.height);
      context.imageSmoothingEnabled = false;
      const top = frame * frameHeight;
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

    const show = (frame: number) => {
      if (!live) return;
      draw(frame);
      if (times.length > 1 && !reducedMotion()) {
        timer = setTimeout(() => {
          show((frame + 1) % times.length);
        }, times[frame]);
      }
    };

    image.onload = () => {
      show(0);
    };
    image.src = src;
    return () => {
      live = false;
      image.onload = null;
      if (timer !== undefined) clearTimeout(timer);
    };
  }, [src, kind, slim, legacy, scale, frameHeight, durations]);

  return (
    <canvas
      ref={canvas}
      className={kind === 'skin' ? styles.skinPreview : styles.capePreview}
      width={figure.w * UNIT}
      height={figure.h * UNIT}
      role="img"
      aria-label={label}
    />
  );
}
