import { useEffect, useId, useState } from 'react';
import { Icon } from '../../components/Icon';
import type { InstanceSummary, ModsCollection, ModsKind } from '../../lib/contracts';
import { ipc } from '../../lib/ipc';
import { numberLocale, strings } from '../../lib/strings';
import { collectionNote, collectionTitle } from './collections';
import { ourPicture } from './helpers';
import styles from './Catalog.module.css';

/** Collections on one page of the strip. */
const PAGE = 3;
/** Icons drawn for a collection; the rest are counted. */
const ICONS = 4;
/** The catalog sends no more; anything past this is not drawn. */
const MAX_COLLECTIONS = 50;

/** Only what the launcher can read: the catalog's answer is bounded by the core too. */
function sane(list: unknown): ModsCollection[] {
  if (!Array.isArray(list)) return [];
  return (list as ModsCollection[])
    .filter(
      (c) =>
        typeof c === 'object' &&
        typeof c.id === 'string' &&
        Number.isInteger(c.count) &&
        c.count > 0 &&
        Array.isArray(c.icons) &&
        Array.isArray(c.names) &&
        typeof c.query === 'object',
    )
    .slice(0, MAX_COLLECTIONS);
}

interface Props {
  kind: ModsKind;
  game: string | null;
  loader: string | null;
  instance: InstanceSummary | null;
  onPick: (collection: ModsCollection) => void;
}

/**
 * The catalog's collections (D-072, adapted from variant 3): tiles of three with
 * icons of what they hold, paged with arrows, or all of them at once. Nothing is
 * drawn when the catalog has none or does not answer.
 */
export function CollectionsStrip({ kind, game, loader, instance, onPick }: Props) {
  const t = strings.mods.catalog;
  const headingId = useId();
  const listId = useId();
  const [answer, setAnswer] = useState<{ key: string; list: ModsCollection[] } | null>(null);
  const [page, setPage] = useState(0);
  const [all, setAll] = useState(false);
  const key = `${kind}|${game ?? ''}|${loader ?? ''}`;

  useEffect(() => {
    let live = true;
    ipc
      .modsCollections(kind, game, loader)
      .then((list) => {
        if (live) setAnswer({ key, list: sane(list) });
      })
      .catch(() => {
        if (live) setAnswer({ key, list: [] });
      });
    return () => {
      live = false;
    };
  }, [kind, game, loader, key]);

  const list = answer?.key === key ? answer.list : [];
  if (list.length === 0) return null;
  const pages = Math.ceil(list.length / PAGE);
  const at = Math.min(page, pages - 1);
  const shown = all ? list : list.slice(at * PAGE, at * PAGE + PAGE);

  return (
    <section className={styles.block} aria-labelledby={headingId}>
      <div className={styles.blockHead}>
        <h3 id={headingId} className={styles.blockTitle}>
          {t.collections}
        </h3>
        <span className={styles.muted}>{t.collectionsHint}</span>
        <span className={styles.spacer} />
        {list.length > PAGE && (
          <button
            type="button"
            className={styles.linkButton}
            aria-expanded={all}
            aria-controls={listId}
            onClick={() => {
              setAll((now) => !now);
            }}
          >
            {all ? t.fewerCollections : t.allCollections}
          </button>
        )}
        {!all && pages > 1 && (
          <span className={styles.pager}>
            <button
              type="button"
              className={[styles.smallButton, styles.iconButton].join(' ')}
              aria-label={t.previousCollections}
              title={t.previousCollections}
              disabled={at === 0}
              onClick={() => {
                setPage(at - 1);
              }}
            >
              <Icon name="prev" size={18} className={styles.mirror} />
            </button>
            <button
              type="button"
              className={[styles.smallButton, styles.iconButton].join(' ')}
              aria-label={t.nextCollections}
              title={t.nextCollections}
              disabled={at >= pages - 1}
              onClick={() => {
                setPage(at + 1);
              }}
            >
              <Icon name="next" size={18} className={styles.mirror} />
            </button>
          </span>
        )}
      </div>
      <ul id={listId} className={styles.collections}>
        {shown.map((collection) => (
          <li key={collection.id}>
            <CollectionTile
              collection={collection}
              fits={
                instance !== null &&
                collection.game !== null &&
                collection.game === instance.versionId &&
                collection.loader !== null &&
                collection.loader === instance.loader?.kind
              }
              onPick={() => {
                onPick(collection);
              }}
            />
          </li>
        ))}
      </ul>
    </section>
  );
}

function CollectionTile({
  collection,
  fits,
  onPick,
}: {
  collection: ModsCollection;
  fits: boolean;
  onPick: () => void;
}) {
  const t = strings.mods.catalog;
  const titleId = useId();
  const noteId = useId();
  const icons = collection.icons
    .map((icon) => ourPicture(icon))
    .filter((icon): icon is string => icon !== null)
    .slice(0, ICONS);
  const more = collection.count - icons.length;
  return (
    <button
      type="button"
      className={styles.collection}
      aria-labelledby={titleId}
      aria-describedby={noteId}
      onClick={onPick}
    >
      <span className={styles.stack} aria-hidden="true">
        {icons.map((icon) => (
          <img key={icon} className={styles.stackIcon} src={icon} alt="" loading="lazy" />
        ))}
        {more > 0 && (
          <span className={styles.stackMore}>+{more.toLocaleString(numberLocale())}</span>
        )}
      </span>
      <span id={titleId} className={styles.collectionName} dir="auto">
        {collectionTitle(collection)}
      </span>
      <span id={noteId} className={styles.collectionNote} dir="auto">
        {collectionNote(collection)}
      </span>
      <span className={styles.collectionFoot}>
        {fits && (
          <span className={[styles.chip, styles.chipSuccess].join(' ')}>{t.fitsInstallation}</span>
        )}
        <span className={styles.spacer} />
        <Icon name="chevr" size={16} className={styles.mirror} />
      </span>
    </button>
  );
}
