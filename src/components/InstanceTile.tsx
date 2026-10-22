import styles from './InstanceTile.module.css';

/** The sizes a tile is drawn at: list rows (32, 40) and the chosen installation (56). */
export type InstanceTileSize = 32 | 40 | 56;

const sizes: Record<InstanceTileSize, string | undefined> = {
  32: styles.small,
  40: styles.medium,
  56: styles.large,
};

const tones = [styles.tone1, styles.tone2, styles.tone3, styles.tone4, styles.tone5, styles.tone6];

/** FNV-1a over the id: the same installation keeps its colors in any list order. */
function toneOf(id: string): string | undefined {
  let hash = 0x811c9dc5;
  for (let i = 0; i < id.length; i += 1) {
    hash ^= id.charCodeAt(i);
    hash = Math.imul(hash, 0x01000193);
  }
  return tones[(hash >>> 0) % tones.length];
}

interface InstanceTileProps {
  /** The installation's id; it picks one of the tile color pairs from the tokens. */
  id: string;
  size?: InstanceTileSize;
  className?: string | undefined;
}

/** A small two-colored block standing for an installation. Decorative: the name says it. */
export function InstanceTile({ id, size = 40, className }: InstanceTileProps) {
  const classes = [styles.tile, sizes[size], toneOf(id), className].filter(Boolean).join(' ');
  return <span className={classes} aria-hidden="true" />;
}
