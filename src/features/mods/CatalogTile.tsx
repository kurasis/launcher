import { useId, useState } from 'react';
import { Icon } from '../../components/Icon';
import { StatusBadge } from '../../components/StatusBadge';
import type { ModsHit } from '../../lib/contracts';
import { strings } from '../../lib/strings';
import { avatarTone, initial } from '../feed/mosaic';
import { compact, ourPicture } from './helpers';
import styles from './Catalog.module.css';

const TONES = [styles.tone0, styles.tone1, styles.tone2, styles.tone3, styles.tone4];

/** A project's icon: the catalog's picture, else a letter on a color of its own. */
export function ProjectIcon({ hit, className }: { hit: ModsHit; className?: string | undefined }) {
  const icon = ourPicture(hit.icon);
  const [broken, setBroken] = useState(false);
  const name = hit.title || hit.slug;
  if (icon !== null && !broken) {
    return (
      <img
        className={className}
        src={icon}
        alt=""
        loading="lazy"
        onError={() => {
          setBroken(true);
        }}
      />
    );
  }
  return (
    <span
      className={[className, styles.letter, TONES[avatarTone(name)]].filter(Boolean).join(' ')}
      aria-hidden="true"
    >
      {initial(name)}
    </span>
  );
}

interface TileProps {
  hit: ModsHit;
  /** Add goes straight to the installation; absent when it cannot (no installation, a modpack). */
  onAdd?: (() => void) | undefined;
  busy?: boolean;
  onOpen: () => void;
}

/**
 * One project in the catalog (D-072, variant 1 "tiles"): its icon, name and author,
 * two lines of its summary, its loaders and downloads. Add and Open show over the
 * loaders while the tile is hovered or holds the focus.
 */
export function CatalogTile({ hit, onAdd, busy = false, onOpen }: TileProps) {
  const t = strings.mods;
  const titleId = useId();
  const names = t.loaderNames as Record<string, string | undefined>;
  const loaders = hit.loaders
    .map((loader) => names[loader])
    .filter((name): name is string => name !== undefined);
  return (
    <article className={styles.tile} aria-labelledby={titleId}>
      <div className={styles.tileHead}>
        <ProjectIcon hit={hit} className={styles.tileIcon} />
        <h4 className={styles.tileTitle}>
          <button
            type="button"
            id={titleId}
            className={styles.tileName}
            dir="auto"
            onClick={onOpen}
          >
            {hit.title || hit.slug}
          </button>
        </h4>
        {hit.author !== null && hit.author !== '' && (
          <span className={styles.author}>{t.by({ author: `\u2068${hit.author}\u2069` })}</span>
        )}
      </div>
      <p className={styles.summary} dir="auto">
        {hit.summary}
      </p>
      <div className={styles.tileFoot}>
        <div className={styles.facts}>
          <span className={styles.loaders}>
            {hit.status === 'importing' ? (
              <StatusBadge tone="warning" label={t.importing} />
            ) : (
              loaders.map((name) => (
                <span key={name} className={styles.loaderChip}>
                  {name}
                </span>
              ))
            )}
          </span>
          <span className={styles.downloads}>
            <Icon name="downloads" size={14} />
            <span aria-hidden="true">{compact(hit.downloads)}</span>
            <span className={styles.visuallyHidden}>
              {t.downloads({ count: compact(hit.downloads) })}
            </span>
          </span>
        </div>
        <div className={styles.actions}>
          {onAdd !== undefined && (
            <button
              type="button"
              className={[styles.smallButton, styles.primarySmall].join(' ')}
              aria-describedby={titleId}
              disabled={busy}
              onClick={onAdd}
            >
              <span className={styles.actionLabel}>{t.catalog.add}</span>
            </button>
          )}
          <button
            type="button"
            className={styles.smallButton}
            aria-describedby={titleId}
            onClick={onOpen}
          >
            <span className={styles.actionLabel}>{t.catalog.open}</span>
          </button>
        </div>
      </div>
    </article>
  );
}

/** Grey tiles while tiles are coming. */
export function TileSkeletons({ count }: { count: number }) {
  return Array.from({ length: count }, (_, index) => (
    <li key={`skeleton-${String(index)}`} aria-hidden="true">
      <div className={styles.skeleton}>
        <span className={styles.skeletonIcon} />
        <span className={styles.skeletonLine} />
        <span className={styles.skeletonText} />
        <span className={styles.skeletonShort} />
      </div>
    </li>
  ));
}
