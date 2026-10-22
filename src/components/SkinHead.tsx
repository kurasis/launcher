import { useCallback, useEffect, useRef, useState } from 'react';
import styles from './SkinHead.module.css';

/** The face of plainSkin() (features/skins/textures.ts), row by row, for a player whose
 * skin is not known: our own plain figure, never the game's default skins. */
const DEFAULT_FACE = [
  'HHHHHHHH',
  'HHHHHHHH',
  'SSSSSSSS',
  'SSSSSSSS',
  'SWPSSPWS',
  'SSSSSSSS',
  'SSSSSSSS',
  'SSSSSSSS',
];

const FACE_COLORS: Record<string, string> = {
  H: '#5c5450',
  S: '#c4a084',
  W: '#fafafa',
  P: '#3c4050',
};

interface SkinHeadProps {
  /** The skin as a data: URL (a 64×64 skin, an HD one, or a vertical strip of frames), or null. */
  texture: string | null;
  /** Pixels on screen; the head is square. */
  size?: number;
  className?: string | undefined;
}

/** The default face as SVG squares: no canvas until a real skin is known. */
function DefaultHead({ size, className }: { size: number; className: string }) {
  return (
    <svg
      className={className}
      viewBox="0 0 8 8"
      width={size}
      height={size}
      shapeRendering="crispEdges"
      aria-hidden="true"
      focusable="false"
    >
      {DEFAULT_FACE.flatMap((row, y) =>
        Array.from({ length: row.length }, (_, x) => row.charAt(x)).map((code, x) => (
          <rect
            key={`${String(x)}-${String(y)}`}
            x={x}
            y={y}
            width={1}
            height={1}
            fill={FACE_COLORS[code]}
          />
        )),
      )}
    </svg>
  );
}

/**
 * The front of a player's head from their skin, the hat layer over the face, the
 * way Minecraft draws it: the avatar of a player everywhere in the launcher.
 */
export function SkinHead({ texture, size = 32, className }: SkinHeadProps) {
  // The texture that would not load or has no head in it: the default face stands in.
  const [failed, setFailed] = useState<string | null>(null);
  const fail = useCallback(() => {
    setFailed(texture);
  }, [texture]);
  const classes = [styles.head, className].filter(Boolean).join(' ');
  if (texture === null || texture === failed) {
    return <DefaultHead size={size} className={classes} />;
  }
  return <TexturedHead texture={texture} size={size} className={classes} onFail={fail} />;
}

function TexturedHead({
  texture,
  size,
  className,
  onFail,
}: {
  texture: string;
  size: number;
  className: string;
  onFail: () => void;
}) {
  const canvas = useRef<HTMLCanvasElement>(null);
  useEffect(() => {
    const context = canvas.current?.getContext('2d') ?? null;
    if (context === null) return;
    context.imageSmoothingEnabled = false;
    let live = true;
    const image = new Image();
    image.onload = () => {
      if (!live) return;
      // A 64 px wide skin is 8 units of 8 px; the face and the hat lie in its first
      // 16 px rows, so old 64×32 skins, HD skins and strips of frames all have them.
      const unit = image.naturalWidth / 8;
      if (!Number.isFinite(unit) || unit <= 0 || image.naturalHeight < unit * 2) {
        onFail();
        return;
      }
      context.clearRect(0, 0, 8, 8);
      context.drawImage(image, unit, unit, unit, unit, 0, 0, 8, 8);
      context.drawImage(image, unit * 5, unit, unit, unit, 0, 0, 8, 8);
    };
    image.onerror = () => {
      if (live) onFail();
    };
    image.src = texture;
    return () => {
      live = false;
    };
  }, [texture, onFail]);
  return (
    <canvas
      ref={canvas}
      className={className}
      width={8}
      height={8}
      style={{ width: size, height: size }}
      aria-hidden="true"
    />
  );
}
