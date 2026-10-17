import { useId } from 'react';
import styles from './HeroArt.module.css';

/** Columns of [height] in 4-unit blocks, left to right, for the two hill layers. */
const FAR = [
  5, 5, 6, 7, 7, 8, 8, 8, 7, 6, 6, 7, 8, 9, 9, 9, 8, 7, 7, 6, 6, 7, 8, 8, 9, 10, 10, 9, 8, 7, 7, 8,
  9, 9, 8, 7, 6, 6, 7, 7,
];
const NEAR = [
  3, 3, 3, 4, 4, 4, 5, 5, 4, 4, 3, 3, 4, 5, 6, 6, 6, 5, 4, 4, 4, 5, 5, 6, 7, 7, 6, 5, 5, 4, 4, 4, 5,
  6, 6, 5, 4, 4, 3, 3,
];
const BLOCK = 4;
const HEIGHT = 60;

function skyline(columns: number[]): string {
  const points = [`0,${String(HEIGHT)}`];
  columns.forEach((blocks, i) => {
    const y = HEIGHT - blocks * BLOCK;
    points.push(`${String(i * BLOCK)},${String(y)}`, `${String((i + 1) * BLOCK)},${String(y)}`);
  });
  points.push(`${String(columns.length * BLOCK)},${String(HEIGHT)}`);
  return points.join(' ');
}

/** Decorative block landscape behind the hero panel. Colors come from theme tokens. */
export function HeroArt({ className }: { className?: string | undefined }) {
  // useId output contains characters that url(#…) references do not accept.
  const sky = `sky${useId().replace(/[^a-zA-Z0-9]/g, '')}`;
  return (
    <svg
      className={[styles.art, className].filter(Boolean).join(' ')}
      viewBox={`0 0 160 ${String(HEIGHT)}`}
      preserveAspectRatio="xMaxYMax slice"
      aria-hidden="true"
      focusable="false"
      shapeRendering="crispEdges"
    >
      <defs>
        <linearGradient id={sky} x1="0" y1="0" x2="0" y2="1">
          <stop offset="0" className={styles.skyTop} />
          <stop offset="1" className={styles.skyBottom} />
        </linearGradient>
      </defs>
      <rect width="160" height={HEIGHT} fill={`url(#${sky})`} />
      <g className={styles.star}>
        <rect x="104" y="6" width="1" height="1" />
        <rect x="118" y="3" width="1" height="1" />
        <rect x="131" y="10" width="1" height="1" />
        <rect x="139" y="4" width="1" height="1" />
        <rect x="156" y="16" width="1" height="1" />
        <rect x="96" y="12" width="1" height="1" />
      </g>
      <rect className={styles.sun} x="147" y="6" width="6" height="6" />
      <g className={styles.cloud}>
        <rect x="112" y="16" width="18" height="3" />
        <rect x="116" y="13" width="9" height="3" />
        <rect x="134" y="24" width="14" height="3" />
      </g>
      <polygon className={styles.far} points={skyline(FAR)} />
      <polygon className={styles.near} points={skyline(NEAR)} />
      <rect className={styles.soil} x="0" y={HEIGHT - 4} width="160" height="4" />
      <g>
        <rect className={styles.trunk} x="97" y="24" width="2" height="8" />
        <rect className={styles.leaves} x="93" y="15" width="10" height="10" />
        <rect className={styles.trunk} x="141" y="32" width="2" height="8" />
        <rect className={styles.leaves} x="137" y="23" width="10" height="10" />
      </g>
    </svg>
  );
}
